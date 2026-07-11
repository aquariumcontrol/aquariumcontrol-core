/* Copyright 2026 Uwe Martin (uwe.martin@gmail.com)

Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated documentation files (the “Software”), to deal in the Software without restriction, including without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED “AS IS”, WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.
*/

/// Spawns an asynchronous Tokio task.
///
/// Depending on whether the `debug_tokio` feature is enabled:
/// - **Enabled (`debug_tokio`):** Spawns the task using `tokio::task::Builder` to associate
///   a tracking name with the task (visible in tools like `tokio-console`). If spawning fails,
///   it converts the error into `StartupError::TokioSpawnFailure` and propagates it using the `?` operator.
/// - **Disabled (`not(debug_tokio)`):** Directly spawns the task using `tokio::spawn` without
///   a name and returns the `JoinHandle` directly.
///
/// # Arguments
///
/// * `$name` - A string slice (`&str` or equivalent expression) specifying the diagnostic label for the task.
/// * `$future` - An asynchronous block/expression representing the execution payload.
///
/// # Errors
///
/// Under the `debug_tokio` feature, this macro contains a `?` operation and will propagate
/// a `StartupError` to the caller if spawning fails. Ensure the enclosing function returns a
/// `Result<_, StartupError>` (or compatible error type).
#[cfg(feature = "debug_tokio")]
#[macro_export]
macro_rules! spawn_tokio_task {
    ($name:expr, $future:expr) => {
        tokio::task::Builder::new()
            .name($name)
            .spawn($future)
            .map_err(|e| StartupError::TokioSpawnFailure {
                location: $name.to_string(),
                source: e
            })?
    };
}

/// Spawns an asynchronous Tokio task.
///
/// Depending on whether the `debug_tokio` feature is enabled:
/// - **Enabled (`debug_tokio`):** Spawns the task using `tokio::task::Builder` to associate
///   a tracking name with the task (visible in tools like `tokio-console`). If spawning fails,
///   it converts the error into `StartupError::TokioSpawnFailure` and propagates it using the `?` operator.
/// - **Disabled (`not(debug_tokio)`):** Directly spawns the task using `tokio::spawn` without
///   a name and returns the `JoinHandle` directly.
///
/// # Arguments
///
/// * `$name` - A string slice (`&str` or equivalent expression) specifying the diagnostic label for the task.
/// * `$future` - An asynchronous block/expression representing the execution payload.
///
/// # Errors
///
/// Under the `debug_tokio` feature, this macro contains a `?` operation and will propagate
/// a `StartupError` to the caller if spawning fails. Ensure the enclosing function returns a
/// `Result<_, StartupError>` (or compatible error type).
#[cfg(not(feature = "debug_tokio"))]
#[macro_export]
macro_rules! spawn_tokio_task {
    ($name:expr, $future:expr) => {
        tokio::spawn($future)
    };
}
