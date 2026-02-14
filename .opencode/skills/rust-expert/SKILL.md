---
name: rust-expert
description: Advanced Rust patterns, focusing on ownership, safety, and performance.
---

# Rust Expert Skill

## 1. Ownership & Lifetimes
- **Borrow Checker First:** Always prefer borrowing (`&T` or `&mut T`) over cloning (`.clone()`) unless the data must be owned.
- **Lifetime Elision:** Do not manually specify lifetimes (e.g., `<'a>`) unless the compiler cannot infer them.
- **Smart Pointers:** Use `Rc<T>` for multiple readers, `Arc<T>` for thread-safe sharing, and `Box<T>` for heap allocation of large structs.

## 2. Error Handling
- **No Panics:** Avoid `unwrap()` and `expect()`. Use `?` for propagation.
- **Result Types:** Prefer the `anyhow` crate for application logic and `thiserror` for library-grade error enums.
- **Context:** Always use `.context("...")` with anyhow to provide a stack-trace-like experience.

## 3. Style & Idioms
- **Pattern Matching:** Use `match` or `if let` instead of nested `if` statements for `Option` and `Result`.
- **Clippy:** Assume `cargo clippy` is active. Write code that passes default linting rules.
- **Functional Style:** Use iterator chains (`.map()`, `.filter()`, `.collect()`) where it improves readability over `for` loops.

## 4. Modern Tooling
- **Async:** Use `tokio` as the default runtime. Use `#[tokio::main]`.
- **Serialization:** Use `serde` with `#[derive(Serialize, Deserialize)]`.

