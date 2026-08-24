// Binary entry point. The full CLI (argument parsing, subcommand-style modes)
// arrives in T2; for the S1 bootstrap this stub simply reports the version so
// the `mutate4rust` bin target builds and runs.
fn main() {
    println!("mutate4rust {}", mutate4rust::version());
}
