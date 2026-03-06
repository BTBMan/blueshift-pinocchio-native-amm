use core::slice::from_ref;

use constant_product_curve::ConstantProduct;
// 根据用户希望销毁的 lp 数量, 提取相对应的 token x 和 token y
use pinocchio::{
    cpi::{Seed, Signer},
    error::ProgramError,
    sysvars::{clock::Clock, Sysvar},
    AccountView, ProgramResult,
};
use pinocchio_token::{
    instructions::{Burn, Transfer},
    state::{Mint, TokenAccount},
};

use crate::state::{AmmState, Config};

pub struct WithdrawAccounts<'a> {
    // 用户即 signer
    user: &'a AccountView,
    mint_lp: &'a AccountView,
    // 池子的相关 token 账户
    vault_x: &'a AccountView,
    vault_y: &'a AccountView,
    // 用户相关的 token ata 账户
    user_x_ata: &'a AccountView,
    user_y_ata: &'a AccountView,
    user_lp_ata: &'a AccountView,
    config: &'a AccountView,
    token_program: &'a AccountView,
}

impl<'a> TryFrom<&'a [AccountView]> for WithdrawAccounts<'a> {
    type Error = ProgramError;

    fn try_from(accounts: &'a [AccountView]) -> Result<Self, Self::Error> {
        let mut account_iter = accounts.into_iter();

        Ok(Self {
            user: account_iter
                .next()
                .ok_or(ProgramError::NotEnoughAccountKeys)?,
            mint_lp: account_iter
                .next()
                .ok_or(ProgramError::NotEnoughAccountKeys)?,
            vault_x: account_iter
                .next()
                .ok_or(ProgramError::NotEnoughAccountKeys)?,
            vault_y: account_iter
                .next()
                .ok_or(ProgramError::NotEnoughAccountKeys)?,
            user_x_ata: account_iter
                .next()
                .ok_or(ProgramError::NotEnoughAccountKeys)?,
            user_y_ata: account_iter
                .next()
                .ok_or(ProgramError::NotEnoughAccountKeys)?,
            user_lp_ata: account_iter
                .next()
                .ok_or(ProgramError::NotEnoughAccountKeys)?,
            config: account_iter
                .next()
                .ok_or(ProgramError::NotEnoughAccountKeys)?,
            token_program: account_iter
                .next()
                .ok_or(ProgramError::NotEnoughAccountKeys)?,
        })
    }
}

#[repr(C, packed)]
#[derive(Copy, Clone)]
pub struct WithdrawInstructionData {
    // 用户希望销魂的 lp 数量
    amount: u64,
    // 用户希望提取的最小 token x/y 数量
    min_x: u64,
    min_y: u64,
    // 过期时间
    expiration: i64,
}

impl TryFrom<&[u8]> for WithdrawInstructionData {
    type Error = ProgramError;

    fn try_from(data: &[u8]) -> Result<Self, Self::Error> {
        if data.len() != size_of::<Self>() {
            return Err(ProgramError::InvalidInstructionData);
        }

        Ok(unsafe { *(data.as_ptr() as *const Self) })
    }
}

pub struct Withdraw<'a> {
    pub accounts: WithdrawAccounts<'a>,
    pub instruction_data: WithdrawInstructionData,
}

impl<'a> TryFrom<(&[u8], &'a [AccountView])> for Withdraw<'a> {
    type Error = ProgramError;

    fn try_from((data, accounts): (&[u8], &'a [AccountView])) -> Result<Self, Self::Error> {
        let accounts = WithdrawAccounts::try_from(accounts)?;
        let instruction_data = WithdrawInstructionData::try_from(data)?;

        Ok(Self {
            accounts,
            instruction_data,
        })
    }
}

impl<'a> Withdraw<'a> {
    pub const DISCRIMINATOR: &'a u8 = &2;

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
        // 如果池子是禁用状态, 则无法进行提取操作
        if config.state() == AmmState::Disabled as u8 {
            return Err(ProgramError::InvalidAccountData);
        }

        // 反序列化账户信息
        let mint_lp = unsafe { Mint::from_account_view_unchecked(accounts.mint_lp)? };
        // 这两个账户可以在指令之外进行创建和初始化
        let vault_x = unsafe { TokenAccount::from_account_view_unchecked(accounts.vault_x)? };
        let vault_y = unsafe { TokenAccount::from_account_view_unchecked(accounts.vault_y)? };

        // 计算提取的 token x 和 token y 的数量
        // 如果用户希望提取的 lp 和总的 lp 储量相等, 则全额提取出
        let (x, y) = if mint_lp.supply() == instruction_data.amount {
            (vault_x.amount(), vault_y.amount())
        } else {
            let amount = ConstantProduct::xy_withdraw_amounts_from_l(
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
        if x < instruction_data.min_x || y < instruction_data.min_y {
            return Err(ProgramError::InvalidArgument);
        }

        // 销毁用户的 lp token
        Burn {
            account: accounts.user_lp_ata,
            mint: accounts.mint_lp,
            authority: accounts.user,
            amount: instruction_data.amount,
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

        // 从池子中提取 token x 和 token y 到 user 账户
        Transfer {
            from: accounts.vault_x,
            to: accounts.user_x_ata,
            authority: accounts.config,
            amount: x,
        }
        .invoke_signed(from_ref(&config_signer))?;

        Transfer {
            from: accounts.vault_y,
            to: accounts.user_y_ata,
            authority: accounts.config,
            amount: y,
        }
        .invoke_signed(&[config_signer])?;

        Ok(())
    }
}
