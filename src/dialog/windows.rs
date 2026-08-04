//! Windows dialogs via PowerShell.
//!
//! Avoids hand-rolling a Win32 dialog template: `Microsoft.VisualBasic`
//! supplies a line-input box, and a small WinForms form covers the masked
//! case, both already present on every supported Windows version.

use std::process::Command;

/// Escapes a value for a PowerShell single-quoted string, where the only
/// metacharacter is the quote itself (doubled to escape).
fn escape(s: &str) -> String {
    s.replace('\'', "''")
}

pub fn available() -> bool {
    true
}

pub fn prompt(title: &str, message: &str, default: &str, secret: bool) -> Option<String> {
    let script = if secret {
        format!(
            r#"Add-Type -AssemblyName System.Windows.Forms,System.Drawing
$f = New-Object System.Windows.Forms.Form
$f.Text = '{title}'; $f.Width = 420; $f.Height = 170; $f.TopMost = $true
$f.FormBorderStyle = 'FixedDialog'; $f.StartPosition = 'CenterScreen'
$l = New-Object System.Windows.Forms.Label
$l.Text = '{message}'; $l.Left = 10; $l.Top = 15; $l.Width = 390; $l.Height = 30
$t = New-Object System.Windows.Forms.TextBox
$t.UseSystemPasswordChar = $true; $t.Left = 10; $t.Top = 55; $t.Width = 390
$ok = New-Object System.Windows.Forms.Button
$ok.Text = 'OK'; $ok.Left = 230; $ok.Top = 90; $ok.DialogResult = 'OK'
$c = New-Object System.Windows.Forms.Button
$c.Text = 'Cancel'; $c.Left = 315; $c.Top = 90; $c.DialogResult = 'Cancel'
$f.Controls.AddRange(@($l, $t, $ok, $c)); $f.AcceptButton = $ok; $f.CancelButton = $c
if ($f.ShowDialog() -eq 'OK') {{ Write-Output $t.Text }} else {{ exit 1 }}"#,
            title = escape(title),
            message = escape(message)
        )
    } else {
        format!(
            r#"Add-Type -AssemblyName Microsoft.VisualBasic
$r = [Microsoft.VisualBasic.Interaction]::InputBox('{message}', '{title}', '{default}')
if ([string]::IsNullOrEmpty($r)) {{ exit 1 }} else {{ Write-Output $r }}"#,
            message = escape(message),
            title = escape(title),
            default = escape(default)
        )
    };

    let output = Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    Some(super::trim_one_newline(
        String::from_utf8_lossy(&output.stdout).into_owned(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escaping_doubles_single_quotes() {
        assert_eq!(escape("it's"), "it''s");
        assert_eq!(escape("plain"), "plain");
    }
}
