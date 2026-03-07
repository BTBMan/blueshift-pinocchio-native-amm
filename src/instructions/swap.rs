// Swap 代币
use crate::state::{AmmState, Config};
use constant_product_curve::{ConstantProduct, LiquidityPair};
use pinocchio::{
    cpi::{Seed, Signer},
    error::ProgramError,
    sysvars::{clock::Clock, Sysvar},
    AccountView, ProgramResult,
};
use pinocchio_token::{instructions::Transfer, state::TokenAccount};

pub struct SwapAccounts<'a> {
    user: &'a AccountView,
    user_x_ata: &'a AccountView,
    user_y_ata: &'a AccountView,
    vault_x: &'a AccountView,
    vault_y: &'a AccountView,
    config: &'a AccountView,
}

impl<'a> TryFrom<&'a [AccountView]> for SwapAccounts<'a> {
    type Error = ProgramError;

    fn try_from(accounts: &'a [AccountView]) -> Result<Self, Self::Error> {
        let mut iter = accounts.iter();

        Ok(Self {
            user: iter.next().ok_or(ProgramError::NotEnoughAccountKeys)?,
            user_x_ata: iter.next().ok_or(ProgramError::NotEnoughAccountKeys)?,
            user_y_ata: iter.next().ok_or(ProgramError::NotEnoughAccountKeys)?,
            vault_x: iter.next().ok_or(ProgramError::NotEnoughAccountKeys)?,
            vault_y: iter.next().ok_or(ProgramError::NotEnoughAccountKeys)?,
            config: iter.next().ok_or(ProgramError::NotEnoughAccountKeys)?,
        })
    }
}
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct SwapInstructionData {
    // 是否是 token x 换 token y, 反之 y 换 x
    is_x: bool,
    // 用户希望换取的另一个代币的数量
    amount: u64,
    // 交换 amount 时原意接收的最小代币数量
    min: u64,
    // 交易过期时间
    expiration: i64,
}

impl TryFrom<&[u8]> for SwapInstructionData {
    type Error = ProgramError;

    fn try_from(data: &[u8]) -> Result<Self, Self::Error> {
        if data.len() != size_of::<Self>() {
            return Err(ProgramError::InvalidInstructionData);
        }

        Ok(unsafe { *(data.as_ptr() as *const Self) })
    }
}

pub struct Swap<'a> {
    pub accounts: SwapAccounts<'a>,
    pub instruction_data: SwapInstructionData,
}

impl<'a> TryFrom<(&[u8], &'a [AccountView])> for Swap<'a> {
    type Error = ProgramError;

    fn try_from((data, accounts): (&[u8], &'a [AccountView])) -> Result<Self, Self::Error> {
        let accounts = SwapAccounts::try_from(accounts)?;
        let instruction_data = SwapInstructionData::try_from(data)?;

        Ok(Self {
            accounts,
            instruction_data,
        })
    }
}

impl<'a> Swap<'a> {
    pub const DISCRIMINATOR: &'a u8 = &3;

    pub fn process(&self) -> ProgramResult {
        let accounts = &self.accounts;
        let instruction_data = &self.instruction_data;

        // 检查交易有效期
        let clock = Clock::get()?;
        if clock.unix_timestamp > instruction_data.expiration {
            return Err(ProgramError::InvalidArgument);
        }

        // 判断池子的状态
        let config = Config::load(accounts.config)?;
        // 如果池子未初始化, 则无法进行 Swap 操作
        if config.state() != AmmState::Initialized as u8 {
            return Err(ProgramError::InvalidAccountData);
        }

        // 反序列化账户信息
        let vault_x = unsafe { TokenAccount::from_account_view_unchecked(accounts.vault_x)? };
        let vault_y = unsafe { TokenAccount::from_account_view_unchecked(accounts.vault_y)? };

        // init swap curve
        let mut curve = ConstantProduct::init(
            vault_x.amount(),
            vault_y.amount(),
            vault_x.amount(),
            config.fee(),
            None,
        )
        .map_err(|_| ProgramError::ArithmeticOverflow)?;

        // 判断是 token x 还是 token y, 拿到对应的枚举值
        // 下面的 swap 方法内部会进行判断从而执行不同的逻辑
        let p = if instruction_data.is_x {
            LiquidityPair::X
        } else {
            LiquidityPair::Y
        };

        let swap_result = curve
            .swap(p, instruction_data.amount, instruction_data.min)
            .map_err(|_| ProgramError::InvalidArgument)?;

        // 验证数量是否正确
        if swap_result.deposit == 0 || swap_result.withdraw == 0 {
            return Err(ProgramError::InvalidArgument);
        }

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

        // 进行 token 转账操作
        if instruction_data.is_x {
            // 用户发送 token x
            Transfer {
                from: accounts.user_x_ata,
                to: accounts.vault_x,
                authority: accounts.user,
                amount: swap_result.deposit,
            }
            .invoke()?;

            // 池子发送 token y
            Transfer {
                from: accounts.vault_y,
                to: accounts.user_y_ata,
                authority: accounts.config,
                amount: swap_result.withdraw,
            }
            .invoke_signed(&[config_signer])?;
        } else {
            // 用户发送 token y
            Transfer {
                from: accounts.user_y_ata,
                to: accounts.vault_y,
                authority: accounts.user,
                amount: swap_result.deposit,
            }
            .invoke()?;

            // 池子发送 token x
            Transfer {
                from: accounts.vault_x,
                to: accounts.user_x_ata,
                authority: accounts.config,
                amount: swap_result.withdraw,
            }
            .invoke_signed(&[config_signer])?;
        }

        Ok(())
    }
}
