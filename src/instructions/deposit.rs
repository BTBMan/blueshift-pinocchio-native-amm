// 存款
// 存入 mint x 或 mint y 代币获取 mint lp
use crate::state::Config;
use constant_product_curve::ConstantProduct;
use pinocchio::{
    cpi::{Seed, Signer},
    error::ProgramError,
    sysvars::{clock::Clock, Sysvar},
    AccountView, ProgramResult,
};
use pinocchio_token::{
    instructions::{MintTo, Transfer},
    state::{Mint, TokenAccount},
};

pub struct DepositAccounts<'a> {
    // 存入代币的用户, 必须是 signer
    pub user: &'a AccountView,
    // lp token 的铸币账户(须要给 user 铸造 lp token)
    pub mint_lp: &'a AccountView,
    // 池中所有存入的 token x 或 token y 的 token account
    pub vault_x: &'a AccountView,
    pub vault_y: &'a AccountView,
    // 用户的存入的 token x 或 token y 的 ata 账户(需要进行转账操作)
    pub user_x_ata: &'a AccountView,
    pub user_y_ata: &'a AccountView,
    // 用户的 lp ata 账户, 需要铸造到这个账户中
    pub user_lp_ata: &'a AccountView,
    // 池子配置信息 config 账户
    pub config: &'a AccountView,
    pub token_program: &'a AccountView,
}

impl<'a> TryFrom<&'a [AccountView]> for DepositAccounts<'a> {
    type Error = ProgramError;

    fn try_from(accounts: &'a [AccountView]) -> Result<Self, Self::Error> {
        let [user, mint_lp, vault_x, vault_y, user_x_ata, user_y_ata, user_lp_ata, config, token_program, _] =
            accounts
        else {
            return Err(ProgramError::NotEnoughAccountKeys);
        };

        Ok(Self {
            user,
            mint_lp,
            vault_x,
            vault_y,
            user_x_ata,
            user_y_ata,
            user_lp_ata,
            config,
            token_program,
        })
    }
}

#[repr(C, packed)]
pub struct DepositInstructionData {
    // 用户希望接收的 lp token 的数量
    pub amount: u64,
    // 用户希望存入的最大的 token x 和 token y 的数量
    pub max_x: u64,
    pub max_y: u64,
    // 此订单过期时间时间(需要在一定时间内完成交易)
    pub expiration: i64,
}

impl TryFrom<&[u8]> for DepositInstructionData {
    type Error = ProgramError;

    fn try_from(data: &[u8]) -> Result<Self, Self::Error> {
        if data.len() != size_of::<Self>() {
            return Err(ProgramError::InvalidInstructionData);
        }

        let amount = u64::from_le_bytes(data[0..8].try_into().unwrap());
        let max_x = u64::from_le_bytes(data[8..16].try_into().unwrap());
        let max_y = u64::from_le_bytes(data[16..24].try_into().unwrap());
        let expiration = i64::from_le_bytes(data[24..32].try_into().unwrap());

        if amount <= 0 || max_x <= 0 || max_y <= 0 {
            return Err(ProgramError::InvalidInstructionData);
        }

        Ok(Self {
            amount,
            max_x,
            max_y,
            expiration,
        })
    }
}

pub struct Deposit<'a> {
    pub accounts: DepositAccounts<'a>,
    pub instruction_data: DepositInstructionData,
}

impl<'a> TryFrom<(&[u8], &'a [AccountView])> for Deposit<'a> {
    type Error = ProgramError;

    fn try_from((data, accounts): (&[u8], &'a [AccountView])) -> Result<Self, Self::Error> {
        let accounts = DepositAccounts::try_from(accounts)?;
        let instruction_data = DepositInstructionData::try_from(data)?;

        Ok(Self {
            accounts,
            instruction_data,
        })
    }
}

impl<'a> Deposit<'a> {
    pub const DISCRIMINATOR: &'a u8 = &1;

    pub fn process(&self) -> ProgramResult {
        let accounts = &self.accounts;
        let instruction_data = &self.instruction_data;

        // 验证交易是否在过期时间内
        let clock = Clock::get()?;
        if clock.unix_timestamp > instruction_data.expiration {
            return Err(ProgramError::InvalidArgument);
        }

        // 判断池子的状态
        let config = Config::load(accounts.config)?;
        // 只要池子不是初始化的状态就报错
        if config.state() != 1 {
            return Err(ProgramError::InvalidAccountData);
        }

        // 反序列化账户信息, 使用 pinocchio 自带的方法提升性能
        let mint_lp = unsafe { Mint::from_account_view_unchecked(accounts.mint_lp)? };
        // 这两个账户可以在指令之外进行创建和初始化
        let vault_x = unsafe { TokenAccount::from_account_view_unchecked(accounts.vault_x)? };
        let vault_y = unsafe { TokenAccount::from_account_view_unchecked(accounts.vault_y)? };

        // 计算存款金额
        // 如果 mint lp 的储量是 0, 则说明是第一次存款(因为没有 mint 过 lp token)
        // 此时 x 和 y 是用户希望存入的最大数量
        let (x, y) = if mint_lp.supply() == 0 {
            (instruction_data.max_x, instruction_data.max_y)
        } else {
            // 根据池子中两种代币的数量, 和用户希望接收的 lp token 数量和当前的流动性(总的的 lp token 储量)计算出 x 和 y 的存款数量
            let amount = ConstantProduct::xy_deposit_amounts_from_l(
                vault_x.amount(),
                vault_y.amount(),
                mint_lp.supply(),
                instruction_data.amount,
                6,
            )
            .map_err(|_| ProgramError::ArithmeticOverflow)?;

            (amount.x, amount.y)
        };

        // 滑点保护
        if x > instruction_data.max_x || y > instruction_data.max_y {
            return Err(ProgramError::InvalidArgument);
        }

        // 将用户的代币金额转账到池子中
        Transfer {
            from: accounts.user_x_ata,
            to: accounts.vault_x,
            authority: accounts.user,
            amount: x,
        }
        .invoke()?;

        Transfer {
            from: accounts.user_y_ata,
            to: accounts.vault_y,
            authority: accounts.user,
            amount: y,
        }
        .invoke()?;

        // 生成 config PDA 账户的 signer
        let seed_binding = config.seed().to_le_bytes();
        let config_bump_binding = config.config_bump();
        let config_seeds = [
            Seed::from(b"config"),
            Seed::from(&seed_binding),
            Seed::from(config.mint_x().as_ref()),
            Seed::from(config.mint_y().as_ref()),
            Seed::from(&config_bump_binding),
        ];
        let config_signer = Signer::from(&config_seeds);

        // mint lp token 给用户
        MintTo {
            mint: accounts.mint_lp,
            account: accounts.user_lp_ata,
            mint_authority: accounts.config,
            amount: instruction_data.amount,
        }
        .invoke_signed(&[config_signer])?;

        Ok(())
    }
}
