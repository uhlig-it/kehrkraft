This document describes the coding conventions used in this project.

# Markdown Conventions

* Put a newline after each heading
* Write headings in Title Case
* Use this document itself as a style guide for Markdown

# YAML Conventions

* Do not add newlines unless absolutely needed
* Use anchors for value re-use
* Write compact maps and arrays if they have only one or two members

# Rust Coding Conventions

* Prefer secrets injected via environment variables
* Environment variables are managed at development time using dotenv; no need to handle that in Rust
