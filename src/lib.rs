#![no_std]

use pinocchio::{
    address::{declare_id, Address},
    entrypoint,
    error::ProgramError,
    nostd_panic_handler, AccountView, ProgramResult,
};

entrypoint!(process_instruction);
nostd_panic_handler!();

pub mod instructions;
pub mod state;

pub use instructions::*;

declare_id!("22222222222222222222222222222222222222222222");

fn process_instruction(
    _program_id: &Address,
    accounts: &[AccountView],
    instruction_data: &[u8],
) -> ProgramResult {
    match instruction_data.split_first() {
        Some((Initialize::DISCRIMINATOR, data)) => {
            Initialize::try_from((data, accounts))?.process()
        }
        Some((Deposit::DISCRIMINATOR, data)) => Deposit::try_from((data, accounts))?.process(),
        // Some((Withdraw::DISCRIMINATOR, data)) => Withdraw::try_from((data, accounts))?.process(),
        // Some((Swap::DISCRIMINATOR, data)) => Swap::try_from((data, accounts))?.process(),
        _ => Err(ProgramError::InvalidInstructionData),
    }
}
