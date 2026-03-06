use pinocchio::{error::ProgramError, AccountView, ProgramResult};

pub struct SwapAccounts<'a> {
    user: &'a AccountView,
}

impl<'a> TryFrom<&'a [AccountView]> for SwapAccounts<'a> {
    type Error = ProgramError;

    fn try_from(accounts: &'a [AccountView]) -> Result<Self, Self::Error> {
        //
    }
}
#[repr(C, packed)]
pub struct SwapInstructionData {
    //
}

impl TryFrom<&[u8]> for SwapInstructionData {
    type Error = ProgramError;

    fn try_from(data: &[u8]) -> Result<Self, Self::Error> {
        //
    }
}

pub struct Swap<'a> {
    pub accounts: SwapAccounts<'a>,
    pub instruction_data: SwapInstructionData,
}

impl<'a> TryFrom<(&[u8], &'a [AccountView])> for Swap<'a> {
    type Error = ProgramError;

    fn try_from((data, accounts): (&[u8], &'a [AccountView])) -> Result<Self, Self::Error> {
        //
    }
}

impl<'a> Swap<'a> {
    pub const DISCRIMINATOR: &'a u8 = &3;

    pub fn process(&self) -> ProgramResult {
        //
    }
}
