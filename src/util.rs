use crate::error::Error;

pub fn bash(cmd: String) -> Result<(), Error> {
    use std::process::Command;
    let status = Command::new("bash").arg("-c").arg(&cmd).status()?;
    if !status.success() {
        return Err(Error::CommandError("bash".to_owned(), cmd).into());
    }
    Ok(())
}
