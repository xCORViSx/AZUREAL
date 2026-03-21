// Quick test: fires a Windows toast notification with icon via PowerShell WinRT.
#[cfg(not(windows))]
fn main() {
    eprintln!("This example only works on Windows.");
}

#[cfg(windows)]
fn main() {
    use std::os::windows::process::CommandExt;

    let title = "AZUREAL";
    let body = "Test notification with icon!";
    let icon_path = dirs::home_dir()
        .unwrap()
        .join(".azureal")
        .join("Azureal_toast.png")
        .to_string_lossy()
        .replace('\\', "/");

    let xml = format!(
        "<toast><visual><binding template=\"ToastGeneric\">\
         <image placement=\"appLogoOverride\" src=\"{icon_path}\"/>\
         <text>{title}</text><text>{body}</text>\
         </binding></visual></toast>"
    );
    let ps = format!(
        "[Windows.UI.Notifications.ToastNotificationManager, Windows.UI.Notifications, ContentType = WindowsRuntime] | Out-Null; \
         [Windows.Data.Xml.Dom.XmlDocument, Windows.Data.Xml.Dom, ContentType = WindowsRuntime] | Out-Null; \
         $xml = New-Object Windows.Data.Xml.Dom.XmlDocument; \
         $xml.LoadXml('{}'); \
         [Windows.UI.Notifications.ToastNotificationManager]::CreateToastNotifier('{{1AC14E77-02E7-4E5D-B744-2EB1AE5198B7}}\\WindowsPowerShell\\v1.0\\powershell.exe').Show(\
         [Windows.UI.Notifications.ToastNotification]::new($xml))",
        xml.replace('\'', "''"),
    );
    match std::process::Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", &ps])
        .creation_flags(0x08000000)
        .output()
    {
        Ok(o) if o.status.success() => println!("Notification sent successfully"),
        Ok(o) => eprintln!("PowerShell failed: {}", String::from_utf8_lossy(&o.stderr)),
        Err(e) => eprintln!("Failed to launch powershell: {e}"),
    }
}
