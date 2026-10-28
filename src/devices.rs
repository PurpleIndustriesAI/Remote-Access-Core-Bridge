//! Bounded USB metadata inspection. Never opens devices or exports serial numbers.
use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Device {
    pub class: String,
    pub name: String,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Inventory {
    pub devices: Vec<Device>,
    pub limit_reached: bool,
}

impl Inventory {
    pub fn validate(&self) -> Result<()> {
        if self.devices.len() > 64 || self.devices.iter().any(|d| {
            [&d.class, &d.name, &d.status].iter().any(|s| s.len() > 512 || s.chars().any(char::is_control))
        }) {
            bail!("device report exceeds the metadata limits");
        }
        Ok(())
    }
}

#[cfg(windows)]
pub fn inventory() -> Result<Inventory> {
    use anyhow::Context;
    use std::io::Read;
    use std::os::windows::process::CommandExt;
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    // Fixed OS query, no peer supplied commands, paths or device identifiers.
    let script = r#"$ErrorActionPreference='Stop'; [Console]::OutputEncoding=[Text.UTF8Encoding]::new(); $rows=@(Get-PnpDevice -PresentOnly | Where-Object {$_.InstanceId.StartsWith('USB\')} | Select-Object -First 65); $items=@($rows | Select-Object -First 64 | ForEach-Object { $model=[regex]::Match($_.InstanceId,'VID_[0-9A-Fa-f]{4}&PID_[0-9A-Fa-f]{4}').Value; @{class=[string]$_.Class; name=$model; status=[string]$_.Status} }); @{devices=$items;limit_reached=($rows.Count -gt 64)} | ConvertTo-Json -Depth 4 -Compress"#;
    let mut child = Command::new(r"C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe")
        .args(["-NoLogo", "-NoProfile", "-NonInteractive", "-Command", script])
        .stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null())
        .creation_flags(0x0800_0000 | 0x0000_4000).spawn()?;
    let stdout = child.stdout.take().context("device query has no output pipe")?;
    let reader = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        stdout.take(65537).read_to_end(&mut bytes).map(|_| bytes)
    });
    let deadline = Instant::now() + Duration::from_secs(12);
    let result = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Ok(status),
            Err(error) => break Err(anyhow::Error::from(error)),
            Ok(None) if Instant::now() >= deadline => break Err(anyhow::anyhow!("device query timed out")),
            Ok(None) => std::thread::sleep(Duration::from_millis(50)),
        }
    };
    if result.is_err() {
        let _ = child.kill();
        let _ = child.wait();
    }
    let bytes = reader.join().map_err(|_| anyhow::anyhow!("device query reader failed"))??;
    if !result?.success() || bytes.len() > 65536 {
        bail!("device query failed or exceeded its output limit");
    }
    let report: Inventory = serde_json::from_slice(&bytes).context("invalid device report")?;
    report.validate()?;
    Ok(report)
}

#[cfg(not(windows))]
pub fn inventory() -> Result<Inventory> {
    bail!("USB inventory is currently supported only on Windows clients")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn private_or_oversized_metadata_is_rejected() {
        assert!(serde_json::from_str::<Device>(r#"{"class":"USB","name":"Phone","status":"OK","serial":"private"}"#).is_err());
        let device = Device { class: "USB".into(), name: "x".repeat(513), status: "OK".into() };
        assert!(Inventory { devices: vec![device], limit_reached: false }.validate().is_err());
    }
}
