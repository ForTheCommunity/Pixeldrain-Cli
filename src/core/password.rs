use anyhow::Result;
use rpassword::ConfigBuilder;

fn password_config() -> rpassword::Config {
    ConfigBuilder::new().password_feedback_mask('*').build()
}

pub fn prompt_password(prompt: &str) -> Result<String> {
    let config = password_config();
    Ok(rpassword::prompt_password_with_config(prompt, config)?)
}
