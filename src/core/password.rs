use anyhow::Result;
use rpassword::ConfigBuilder;

fn password_config() -> rpassword::Config {
    ConfigBuilder::new().password_feedback_mask('*').build()
}

pub fn prompt_password(prompt: &str) -> Result<String> {
    let config = password_config();
    Ok(rpassword::prompt_password_with_config(prompt, config)?)
}

/// Returns the value of an environment variable if it is set and non-empty.
pub fn env_non_empty(var: &str) -> Option<String> {
    std::env::var(var).ok().filter(|v| !v.is_empty())
}

/// Resolves a secret from an explicit CLI value, then an environment variable,
/// falling back to an interactive password prompt.
pub fn resolve_secret(cli_value: Option<&str>, env_var: &str, prompt: &str) -> Result<String> {
    if let Some(val) = cli_value {
        return Ok(val.to_string());
    }
    if let Some(val) = env_non_empty(env_var) {
        return Ok(val);
    }
    prompt_password(prompt)
}

/// Resolves a secret for creation/setup flows, prompting twice for confirmation
/// only when falling back to interactive input.
pub fn resolve_secret_confirmed(
    cli_value: Option<&str>,
    env_var: &str,
    prompt: &str,
    confirm_prompt: &str,
    mismatch_err: &str,
) -> Result<String> {
    if let Some(val) = cli_value {
        return Ok(val.to_string());
    }
    if let Some(val) = env_non_empty(env_var) {
        return Ok(val);
    }
    let pwd = prompt_password(prompt)?;
    let confirm = prompt_password(confirm_prompt)?;
    if pwd != confirm {
        return Err(anyhow::anyhow!("{}", mismatch_err));
    }
    Ok(pwd)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_secret_prefers_cli_over_env() {
        let var = "PIXELDRAIN_TEST_SECRET_PREF";
        // SAFETY: unique env var name used only by this test.
        unsafe { std::env::set_var(var, "from_env") };
        let val = resolve_secret(Some("from_cli"), var, "prompt: ").unwrap();
        assert_eq!(val, "from_cli");

        let val_env = resolve_secret(None, var, "prompt: ").unwrap();
        assert_eq!(val_env, "from_env");
        // SAFETY: cleanup unique test env var.
        unsafe { std::env::remove_var(var) };
    }

    #[test]
    fn test_resolve_secret_confirmed_uses_cli_or_env() {
        let var = "PIXELDRAIN_TEST_CONFIRMED";
        // SAFETY: unique env var name used only by this test.
        unsafe { std::env::set_var(var, "env_master") };
        assert_eq!(
            resolve_secret_confirmed(Some("cli_master"), var, "p: ", "c: ", "mismatch").unwrap(),
            "cli_master"
        );
        assert_eq!(
            resolve_secret_confirmed(None, var, "p: ", "c: ", "mismatch").unwrap(),
            "env_master"
        );
        // SAFETY: cleanup unique test env var.
        unsafe { std::env::remove_var(var) };
    }

    #[test]
    fn test_env_non_empty_ignores_empty_string() {
        let var = "PIXELDRAIN_TEST_EMPTY";
        // SAFETY: unique env var name used only by this test.
        unsafe { std::env::set_var(var, "") };
        assert_eq!(env_non_empty(var), None);
        // SAFETY: cleanup unique test env var.
        unsafe { std::env::remove_var(var) };
    }
}
