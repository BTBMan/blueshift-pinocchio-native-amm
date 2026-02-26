use pinocchio::{error::ProgramError, Address, ProgramResult};
use solana_address::address_eq;

// 此结构体使用了零填充, 消除了对齐要求, 所有内容都以 1 对齐, 因为除了 state 以外, 所有字段都是 [u8; N]
#[repr(C)]
pub struct Config {
    // AMM 当前的状态(未初始化, 初始化, 已禁用, 仅限提取等)
    state: u8,
    // PDA 的 seed, 使用数组是为了保证内存对齐
    seed: [u8; 8],
    // 对 AMM 拥有控制权的账户
    authority: Address,
    // token x 的 mint 地址
    mint_x: Address,
    // token y 的 mint 地址
    mint_y: Address,
    // 以基点表示 swap 的 fee (1基点 = 0.01%), 使用数组的目的和 seed 的目的是一样的
    fee: [u8; 2],
    // 缓存的 bump
    config_bump: [u8; 1],
}

// repr(u8) 保证了枚举使用 1 个字节来存储, 防止 rust 自动设置存储的字节大小
#[repr(u8)]
pub enum AmmState {
    Uninitialized,
    Initialized,
    Disabled,
    WithdrawOnly,
}

// 实现 Config 方法
impl Config {
    pub const LEN: usize = size_of::<Config>();

    // Related functions
    #[inline(always)]
    pub fn a() {}

    // 从字节切片创建 Config 的引用, 不进行边界检查
    #[inline(always)]
    pub fn from_bytes_unchecked(bytes: &[u8]) -> &Self {
        unsafe { &*(bytes.as_ptr() as *const Self) }
    }

    // Utils
    #[inline(always)]
    pub fn has_authority(&self) -> Option<Address> {
        // let ptr: *const Address = &raw const self.authority;
        // let authority = self.authority();
        // 转换为 u8 数组, 进行对比的时候须要 32 个逐个对比
        // let auth1 = unsafe { &*(authority.to_bytes().as_ptr() as *const [u8; 32]) };
        //
        // 转换为 u64 数组, SIMD(单指令多数据; Single Instruction Multiple Data) 友好, 性能更高(一条 CPU 指令处理多个数据)
        // auth1[0] 包涵 bytes[0..8], 一次操作既可验证 8 个字节, 以此类推总共只需 4 此操作
        // let auth2 = unsafe { &*(authority.to_bytes().as_ptr() as *const [u64; 4]) };
        //
        // 使用 read_unaligned 直接把内容拷贝了一份到栈上, 后续读取的这个值和原来的值地址无关
        let auth = unsafe { core::ptr::addr_of!(self.authority).read_unaligned() };
        let default_address = Address::default();

        // 如果 authority 是默认地址, 说明没有设置 authority
        // address_eq SIMD 优化
        if address_eq(&auth, &default_address) {
            None
        } else {
            Some(auth)
        }
    }

    // Getters
    #[inline(always)]
    pub fn state(&self) -> u8 {
        self.state
    }

    #[inline(always)]
    pub fn seed(&self) -> u64 {
        u64::from_le_bytes(self.seed)
    }

    #[inline(always)]
    pub fn authority(&self) -> &Address {
        &self.authority
    }

    #[inline(always)]
    pub fn mint_x(&self) -> &Address {
        &self.mint_x
    }

    #[inline(always)]
    pub fn mint_y(&self) -> &Address {
        &self.mint_y
    }

    #[inline(always)]
    pub fn fee(&self) -> u16 {
        u16::from_le_bytes(self.fee)
    }

    #[inline(always)]
    pub fn config_bump(&self) -> [u8; 1] {
        self.config_bump
    }

    // Setters
    #[inline(always)]
    pub fn set_state(&mut self, state: u8) -> ProgramResult {
        // 1. 验证合法范围
        // 2. WithdrawOnly 指令特殊, 不可以通过 set_state 方法设置它
        if state.ge(&(AmmState::WithdrawOnly as u8)) {
            return Err(ProgramError::InvalidAccountData);
        }

        self.state = state;

        Ok(())
    }

    #[inline(always)]
    pub fn set_seed(&mut self, seed: u64) {
        self.seed = seed.to_le_bytes();
    }

    #[inline(always)]
    pub fn set_authority(&mut self, authority: Address) {
        self.authority = authority;
    }

    #[inline(always)]
    pub fn set_mint_x(&mut self, mint_x: Address) {
        self.mint_x = mint_x;
    }

    #[inline(always)]
    pub fn set_mint_y(&mut self, mint_y: Address) {
        self.mint_y = mint_y;
    }

    #[inline(always)]
    pub fn set_fee(&mut self, fee: u16) -> ProgramResult {
        // fee 率不能 >= 100%
        if fee.ge(&10_000) {
            return Err(ProgramError::InvalidAccountData);
        }

        self.fee = fee.to_le_bytes();

        Ok(())
    }

    #[inline(always)]
    pub fn set_config_bump(&mut self, config_bump: [u8; 1]) {
        self.config_bump = config_bump;
    }

    #[inline(always)]
    pub fn set_inner(
        &mut self,
        state: u8,
        seed: u64,
        authority: Address,
        mint_x: Address,
        mint_y: Address,
        fee: u16,
        config_bump: [u8; 1],
    ) -> ProgramResult {
        self.set_state(state)?;
        self.set_seed(seed);
        self.set_authority(authority);
        self.set_mint_x(mint_x);
        self.set_mint_y(mint_y);
        self.set_fee(fee)?;
        self.set_config_bump(config_bump);

        Ok(())
    }
}
