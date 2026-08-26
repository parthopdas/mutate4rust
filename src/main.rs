// Binary entry point — a thin outer adapter.
//
// All argument parsing and dispatch lives in the `cli` adapter module; `main`
// only forwards the resulting process exit code (see the exit-code contract in
// `mutate4rust::cli`).
use std::process::ExitCode;

use mutate4rust::cli::Cli;

fn main() -> ExitCode {
    Cli::main()
}
