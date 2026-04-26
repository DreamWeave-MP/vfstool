// SPDX-License-Identifier: GPL-3.0-only
pub enum VFSToolExitCode {
    FindFailed = 1,
    FileNotInLooseDirectories = 2,
    DriftDetected = 4,
    BadRegex = 6,
    FailedToLoadOpenMWConfig = 7,
    InvalidInput = 8,
    RuntimeFailure = 9,
}

impl From<VFSToolExitCode> for i32 {
    fn from(value: VFSToolExitCode) -> Self {
        match value {
            VFSToolExitCode::FindFailed => 1,
            VFSToolExitCode::FileNotInLooseDirectories => 2,
            VFSToolExitCode::DriftDetected => 4,
            VFSToolExitCode::BadRegex => 6,
            VFSToolExitCode::FailedToLoadOpenMWConfig => 7,
            VFSToolExitCode::InvalidInput => 8,
            VFSToolExitCode::RuntimeFailure => 9,
        }
    }
}
