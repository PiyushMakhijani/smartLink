# Manual smoke run: exercises junctions, symlinks, hardlinks and DEAD detection
# against real NTFS. Usage: pwsh -File scripts\smoke.ps1
$exe = Join-Path $PSScriptRoot '..\target\debug\smartlink.exe'
$tmp = Join-Path $env:TEMP 'smartlink-smoke'
Remove-Item $tmp -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Path "$tmp\libs\mylib", "$tmp\proj\vendor", "$tmp\proj\src" | Out-Null
Set-Content "$tmp\libs\mylib\api.rs" 'fn main() {}'
Set-Content "$tmp\proj\src\shared.txt" 'hello'

'--- junction (dir, no elevation needed) ---'
& $exe make "$tmp\libs\mylib" "$tmp\proj\vendor\mylib" --junction
'--- relative symlink (needs Developer Mode) ---'
& $exe make "$tmp\proj\src\shared.txt" "$tmp\proj\vendor\shared-link.txt" --symlink --relative 2>&1
'--- auto-detect (same-volume file -> hardlink) ---'
& $exe make "$tmp\proj\src\shared.txt" "$tmp\proj\vendor\hard.txt"
'--- DEAD row: junction whose target is deleted afterwards ---'
New-Item -ItemType Directory -Path "$tmp\libs\doomed" | Out-Null
& $exe make "$tmp\libs\doomed" "$tmp\proj\vendor\doomed" --junction | Out-Null
cmd /c rmdir "$tmp\libs\doomed"
'--- ls table ---'
& $exe ls "$tmp\proj\vendor" --hardlinks
