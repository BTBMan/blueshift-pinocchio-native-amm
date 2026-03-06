use crate::state::Config;
use core::{
    mem::MaybeUninit,
    ptr::{copy_nonoverlapping, write_bytes},
};
use pinocchio::{
    cpi::{Seed, Signer},
    error::ProgramError,
    sysvars::{rent::Rent, Sysvar},
    AccountView, Address, ProgramResult,
};
use pinocchio_system::instructions::CreateAccount;
use pinocchio_token::{instructions::InitializeMint2, state::Mint};

/**
 * 初始化 Config 账户, 存储 AMM 所需的信息
 * 创建 LP mint 账户(mint_lp)
 */

// 初始化 config 账户所需要的账户
pub struct InitializeAccounts<'a> {
    // config 账户的创建者 signer
    pub initializer: &'a AccountView,
    // LP Token mint 账户
    pub mint_lp: &'a AccountView,
    // config 账户
    pub config: &'a AccountView,
}

impl<'a> TryFrom<&'a [AccountView]> for InitializeAccounts<'a> {
    type Error = ProgramError;

    fn try_from(accounts: &'a [AccountView]) -> Result<Self, Self::Error> {
        let [initializer, mint_lp, config, _] = accounts else {
            return Err(ProgramError::NotEnoughAccountKeys);
        };

        Ok(Self {
            initializer,
            mint_lp,
            config,
        })
    }
}

// 初始化 config 账户所需的指令参数
// 按顺序压缩数据(不进行 padding 填充)
#[repr(C, packed)]
pub struct InitializeInstructionData {
    // 创建 config PDA 账户的种子
    pub seed: u64,
    // 池子的基点 fee 率
    pub fee: u16,
    // token x 和 token y 的 mint 账户
    pub mint_x: Address,
    pub mint_y: Address,
    // config 账户和 lp 账户的 bump
    pub config_bump: u8,
    pub lp_bump: u8,
    // config 账户的 authority
    pub authority: Address,
}

impl TryFrom<&[u8]> for InitializeInstructionData {
    type Error = ProgramError;

    fn try_from(data: &[u8]) -> Result<Self, Self::Error> {
        // InitializeInstructionData 数据的长度
        const INITIALIZE_DATA_LEN_WITH_AUTHORITY: usize = size_of::<InitializeInstructionData>();
        // 不包括 authority 的数据长度
        const INITIALIZE_DATA_LEN: usize =
            INITIALIZE_DATA_LEN_WITH_AUTHORITY - size_of::<Address>();

        // 匹配 data 的数据是否有 authority
        // authority 是可选的, 如果不传, 则创建一个不可变的池子
        // 这样可以节省 32 字节的交易数据
        match data.len() {
            INITIALIZE_DATA_LEN_WITH_AUTHORITY => {
                // 直接读取 data 为结构体数据
                Ok(unsafe { (data.as_ptr() as *const Self).read_unaligned() })
            }
            INITIALIZE_DATA_LEN => {
                // 在 data 的末尾手动添加 32 字节的 0

                // 让编译器在栈上生成一块 INITIALIZE_DATA_LEN_WITH_AUTHORITY 大小的内存
                // 先不要初始化它, 我们手动填充数据
                let mut raw: MaybeUninit<[u8; INITIALIZE_DATA_LEN_WITH_AUTHORITY]> =
                    MaybeUninit::uninit();
                // 拿到这块内存的可写指针
                let raw_ptr = raw.as_mut_ptr() as *mut u8;

                unsafe {
                    // 把 data 的前 INITIALIZE_DATA_LEN 大小的数据拷贝进刚刚生成的那块内存中
                    // 因为两块内存完全独立, 不存在重叠, 所以可以安全的使用 copy_nonoverlapping, 可以获得更快的速度
                    copy_nonoverlapping(data.as_ptr(), raw_ptr, INITIALIZE_DATA_LEN);
                    // 手动从 INITIALIZE_DATA_LEN 位置开始写入 32 字节的 0
                    write_bytes(raw_ptr.add(INITIALIZE_DATA_LEN), 0, 32);
                    // 读取 raw_ptr 为结构体数据
                    Ok((raw_ptr as *const Self).read_unaligned())
                }
            }
            _ => Err(ProgramError::InvalidInstructionData),
        }
    }
}

pub struct Initialize<'a> {
    pub accounts: InitializeAccounts<'a>,
    pub instruction_data: InitializeInstructionData,
}

impl<'a> TryFrom<(&'a [u8], &'a [AccountView])> for Initialize<'a> {
    type Error = ProgramError;

    fn try_from((data, accounts): (&'a [u8], &'a [AccountView])) -> Result<Self, Self::Error> {
        let accounts = InitializeAccounts::try_from(accounts)?;
        let instruction_data = InitializeInstructionData::try_from(data)?;

        Ok(Self {
            accounts,
            instruction_data,
        })
    }
}

impl<'a> Initialize<'a> {
    pub const DISCRIMINATOR: &'a u8 = &0;

    pub fn process(&self) -> ProgramResult {
        let instruction_data = &self.instruction_data;
        let accounts = &self.accounts;
        let rent = Rent::get()?;

        // 创建 config 账户
        let config_lamports = rent.try_minimum_balance(Config::LEN)?;
        // 须要 PDA 种子签名才能创建 PDA 账户
        // 其中需要使用 token x mint 和 token y mint 的地址作为种子
        // 以这对 token pair 确定池子配置的唯一性
        let seed_binding = instruction_data.seed.to_le_bytes();
        let config_bump_binding = instruction_data.config_bump.to_le_bytes();
        let config_seeds = [
            Seed::from(b"config"),
            Seed::from(&seed_binding),
            Seed::from(instruction_data.mint_x.as_ref()),
            Seed::from(instruction_data.mint_y.as_ref()),
            Seed::from(&config_bump_binding),
        ];
        let config_signer = Signer::from(&config_seeds);

        CreateAccount {
            from: accounts.initializer,
            to: accounts.config,
            lamports: config_lamports,
            space: Config::LEN as u64,
            owner: &crate::ID,
        }
        .invoke_signed(&[config_signer])?;

        // 将数据填充到 config 账户中
        let config_account = Config::load_unchecked_mut(accounts.config)?;
        config_account.set_inner(
            instruction_data.seed,
            instruction_data.authority.clone(),
            instruction_data.mint_x.clone(),
            instruction_data.mint_y.clone(),
            instruction_data.fee,
            config_bump_binding,
        )?;

        // 创建 LP mint 账户
        // 首先创建一个普通的账户
        let lp_bump_binding = instruction_data.lp_bump.to_le_bytes();
        // 其中需要使用 config 账户地址作为种子, 确定池子的 lp token 的唯一性
        let mint_lp_seeds = [
            Seed::from(b"mint_lp"),
            Seed::from(self.accounts.config.address().as_ref()),
            Seed::from(&lp_bump_binding),
        ];
        let mint_lp_signer = Signer::from(&mint_lp_seeds);
        // 计算创建 mint lp 所需的 lamports
        let mint_lp_lamports = rent.try_minimum_balance(Mint::LEN)?;

        CreateAccount {
            from: accounts.initializer,
            to: accounts.mint_lp,
            lamports: mint_lp_lamports,
            space: Mint::LEN as u64,
            owner: &pinocchio_token::ID,
        }
        .invoke_signed(&[mint_lp_signer])?;

        // 使用 InitializeMint2 指令初始化 mint lp 账户
        InitializeMint2 {
            mint: accounts.mint_lp,
            decimals: 6, // 根据目前 lp token 的小数位数
            mint_authority: accounts.config.address(),
            freeze_authority: None,
        }
        .invoke()?;

        Ok(())
    }
}
