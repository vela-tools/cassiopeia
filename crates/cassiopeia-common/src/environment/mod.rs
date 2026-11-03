//! Values a configuration document reads from the process environment: the `${NAME}` templates that
//! name them, the secrets they resolve to, and the masking that keeps those secrets out of output.

pub mod registry;
pub mod secret;
pub mod template;
pub mod variable_name;
