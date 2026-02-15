---
name: gtk4-rs-master
description: Patterns for GTK4 and Adwaita in Rust, specifically handling UI state and signals.
---

# GTK4 Rust Specialist

## 1. Signal Handling & Closures
- **Use `glib::clone!`**: Always use the `clone!` macro when passing widgets or state into signal handlers (e.g., `button.connect_clicked(clone!(@weak label => move |_| ...))`).
- **Weak References**: Always prefer `@weak` references for widgets in closures to avoid reference cycles and memory leaks.

## 2. State Management
- **Interior Mutability**: Use `Rc<RefCell<T>>` for shared application state that needs to be modified from UI signals.
- **Properties**: For complex widgets, prefer `glib::Properties` and `glib::Object` subclassing over raw structs where appropriate.

## 3. UI Construction
- **GTK4 Defaults**: Use `gtk::Application` and `gtk::ApplicationWindow`. Do not use `gtk::main()` (GTK3 style).
- **Adwaita**: If the project uses `libadwaita`, prefer `adw::Application` and `adw::Window` for a modern GNOME look.
- **Composition**: Prefer using `.ui` files (XML) with `gtk::Builder` or composite templates (`CompositeTemplate`) for complex layouts.

## 4. Layout & Widgets
- **No `add()`**: Remember that `gtk::Container` is gone. Use `.set_child()` or specific methods like `box.append()`.
- **String Handling**: Use `.to_string()` or `.as_str()` explicitly when passing Rust strings to GTK methods.

