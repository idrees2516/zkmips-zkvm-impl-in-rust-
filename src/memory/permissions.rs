use std::ops::{BitAnd, BitOr};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AccessPermissions {
    bits: u8,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Permission {
    None = 0b000,
    Read = 0b001,
    Write = 0b010,
    Execute = 0b100,
}

impl AccessPermissions {
    pub const NONE: AccessPermissions = AccessPermissions { bits: 0b000 };
    pub const READ: AccessPermissions = AccessPermissions { bits: 0b001 };
    pub const WRITE: AccessPermissions = AccessPermissions { bits: 0b010 };
    pub const EXECUTE: AccessPermissions = AccessPermissions { bits: 0b100 };
    pub const READ_WRITE: AccessPermissions = AccessPermissions { bits: 0b011 };
    pub const READ_EXECUTE: AccessPermissions = AccessPermissions { bits: 0b101 };
    pub const WRITE_EXECUTE: AccessPermissions = AccessPermissions { bits: 0b110 };
    pub const ALL: AccessPermissions = AccessPermissions { bits: 0b111 };

    pub fn new(read: bool, write: bool, execute: bool) -> Self {
        let mut bits = 0;
        if read { bits |= Self::READ.bits; }
        if write { bits |= Self::WRITE.bits; }
        if execute { bits |= Self::EXECUTE.bits; }
        Self { bits }
    }

    pub fn has_permission(self, permission: Permission) -> bool {
        (self.bits & permission as u8) == permission as u8
    }

    pub fn add_permission(&mut self, permission: Permission) {
        self.bits |= permission as u8;
    }

    pub fn remove_permission(&mut self, permission: Permission) {
        self.bits &= !(permission as u8);
    }

    pub fn can_read(self) -> bool {
        self.has_permission(Permission::Read)
    }

    pub fn can_write(self) -> bool {
        self.has_permission(Permission::Write)
    }

    pub fn can_execute(self) -> bool {
        self.has_permission(Permission::Execute)
    }

    pub fn as_bits(self) -> u8 {
        self.bits
    }
}

impl Default for AccessPermissions {
    fn default() -> Self {
        Self::READ_WRITE
    }
}

impl BitAnd for AccessPermissions {
    type Output = Self;

    fn bitand(self, rhs: Self) -> Self::Output {
        Self {
            bits: self.bits & rhs.bits
        }
    }
}

impl BitOr for AccessPermissions {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self::Output {
        Self {
            bits: self.bits | rhs.bits
        }
    }
}

impl From<u8> for AccessPermissions {
    fn from(bits: u8) -> Self {
        Self { bits: bits & 0b111 }
    }
}

impl std::fmt::Display for AccessPermissions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}{}{}",
            if self.can_read() { "r" } else { "-" },
            if self.can_write() { "w" } else { "-" },
            if self.can_execute() { "x" } else { "-" }
        )
    }
}
