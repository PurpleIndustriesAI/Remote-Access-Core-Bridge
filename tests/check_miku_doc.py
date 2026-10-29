"""Validate PowerShell examples in the MIKU guide without executing them."""
from pathlib import Path
import re
import subprocess

DOC = Path(__file__).resolve().parents[1] / "docs" / "miku-setup.md"
PARSER = r"""
$tokens = $null
$errors = $null
[System.Management.Automation.Language.Parser]::ParseInput(
    [Console]::In.ReadToEnd(), [ref]$tokens, [ref]$errors
) > $null
if ($errors.Count) {
    $errors | ForEach-Object { Write-Error $_.Message }
    exit 1
}
"""

blocks = re.findall(r"```powershell\n(.*?)```", DOC.read_text(encoding="utf-8"), re.S)
assert blocks, "No PowerShell examples found"
for index, block in enumerate(blocks, 1):
    result = subprocess.run(
        ["powershell", "-NoProfile", "-Command", PARSER],
        input=block,
        text=True,
        capture_output=True,
        check=False,
    )
    if result.returncode:
        raise SystemExit(f"PowerShell block {index} failed:\n{result.stderr}")
print(f"PowerShell syntax passed for {len(blocks)} MIKU guide blocks")
