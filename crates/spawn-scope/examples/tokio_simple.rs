use std::time::Duration;
use tokio::time;
use spawn_scope::scope::{Scope, ScopedSpawn};

#[tokio::main]
async fn main() {
    println!("Starting main function...");

    let scope = Scope::new();

    // Spawn a task that prints a message and then waits.
    scope.spawn(async {
        println!("Task 1: Hello from a spawned task!");
        time::sleep(Duration::from_secs(2)).await;
        println!("Task 1: Task completed after 2 seconds.");
    });

    // Spawn another task that prints a message and might be cancelled.
    scope.spawn_with_hooks(
        async {
            println!("Task 2: Hello from another spawned task!");
            time::sleep(Duration::from_secs(5)).await;
            println!("Task 2: Task completed after 5 seconds.");
        },
        || println!("Task 2: Completed hook called."),
        || println!("Task 2: Cancellation hook called."),
    );

    // Main function waits for a bit.
    time::sleep(Duration::from_secs(1)).await;
    println!("Main function: 1 second elapsed, dropping scope.");

    // The scope is dropped here, which will cancel the spawned tasks.
    // Task 1, having a 2-second sleep, will be cancelled before completion.
    // Task 2, having a 5-second sleep, will also be cancelled before completion.
    drop(scope);

    println!("Main function: Scope dropped. Waiting for a moment to observe cancellations...");
    time::sleep(Duration::from_millis(100)).await;

    println!("Main function: Exiting.");
}

