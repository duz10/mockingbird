$ErrorActionPreference = 'Stop'
$preamble = @"
pub mod error {
    #[derive(Debug)]
    pub enum AppError { ActivitySampler(String) }
    impl std::fmt::Display for AppError {
        fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            match self { Self::ActivitySampler(s) => write!(f, "activity sampler: {}", s) }
        }
    }
    impl std::error::Error for AppError {}
    pub type AppResult<T> = Result<T, AppError>;
}
"@
$deps = @(
    'serde = { version = "1", features = ["derive"] }',
    'serde_json = "1"',
    'windows = { version = "0.56", features = ["Win32_Foundation", "Win32_System_SystemInformation", "Win32_UI_Input_KeyboardAndMouse"] }'
)
& (Join-Path $PSScriptRoot 'throwaway-test.ps1') `
    -ModuleName activity_level `
    -SourcePath (Join-Path (Split-Path $PSScriptRoot -Parent) 'src-tauri\src\activity\activity_level.rs') `
    -Dependencies $deps `
    -Preamble $preamble
