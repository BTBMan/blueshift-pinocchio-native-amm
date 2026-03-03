use core::{
    mem::MaybeUninit,
    ptr::{copy_nonoverlapping, write_bytes},
};
use pinocchio::{error::ProgramError, AccountView, Address};

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

        // 创建 config 账户

        // 创建 LP mint 账户

        Ok(Self {
            accounts,
            instruction_data,
        })
    }
}
