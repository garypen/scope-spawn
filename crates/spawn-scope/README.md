# Spawn Scope

A small utility to manage spawned tokio tasks.

## Usage

```rust
use spawn_scope::scope::Scope;
use spawn_scope::scope::ScopedSpawn;

#[tokio::main]
async fn main() {
    let scope = Scope::new();
    scope.spawn(async {
        println!("Hello from a spawned task!");
    });
    // scope is dropped here, and spawned tasks are cancelled.
}
```

## Examples

A simple example demonstrating basic usage with `tokio` can be found in `examples/tokio_simple.rs`. To run it:

```bash
cargo run --example tokio_simple
```

## Testing

To run the tests, use the following command:
```bash
cargo test
```
