smartlink - Project Plan
1. Vision & Mission
Mission: To provide a unified, cross-platform CLI utility that abstracts the complexity of filesystem links (Symlinks, Hardlinks, Junctions, Bind Mounts) for developers.
Goal: Eliminate broken links during project migration, team onboarding, and OS switching by using intelligent auto-detection, relative pathing, and Git-integrated manifests.
2. Core Features Roadmap
Phase 1: The Foundation (MVP)
Focus: Basic creation and detection across Windows, macOS, and Linux.
smartlink make: Create links with auto-detection.
Logic: If same drive/partition → Hardlink (files). If different → Relative Symlink.
Flags: --hard, --relative, --junction (Windows/Linux Bind), --force.
smartlink ls: Visualize links in a directory.
Output: Table format showing Type, Status (OK/Dead), Name, and Target.
Feature: Color-coded output (Green for OK, Red for Dead).
Feature: Group hardlinks by Inode/FileID.
Cross-Platform Abstraction Layer:
Implement OS-specific wrappers for:
Windows: mklink /H (Hard), mklink (Symlink), mklink /J (Junction).
Linux/macOS: ln (Hard/Symlink), mount --bind (Bind Mount).
Phase 2: The Manager (Conversion & Maintenance)
Focus: Fixing existing messes and changing link types.
smartlink convert: Morph one link type into another.
Logic: Read target → Delete old link → Create new link.
Handling: Prompt user if converting Hardlink to Symlink (multiple targets exist).
Safety: Prevent conversion if target is missing (unless --preserve-broken is used).
smartlink fix: Auto-heal broken links.
Logic: Scan for dead links → Search local directory for matching filenames/hashes → Re-link.
smartlink dedupe: Copy-on-Write (CoW) cloning.
Logic: Use native FS APIs (clonefile, FICLONE, FSCTL_DUPLICATE_EXTENTS) to save space without creating visible links.
Phase 3: The Git Integration (Manifests)
Focus: Persistence across clones and team environments.
Manifest System (.smartlink.json):
Schema: Define links with source, target, type, and os_specific overrides.
smartlink init: Generate manifest from current directory state.
smartlink sync: Recreate all links defined in the manifest.
Git Hooks Integration:
Auto-run smartlink sync on post-checkout and post-merge.
smartlink git check: CI/CD tool to fail builds if absolute symlinks are committed.
3. Technical Architecture
Tech Stack
Language: Rust (Recommended for performance, single-binary distribution, and strong type safety for filesystem ops) OR Go.
CLI Framework: clap (Rust) or cobra (Go).
Filesystem Ops:
Rust: std::fs, pathdiff, junction crate, nix crate (for Linux mounts).
Go: os, filepath, golang.org/x/sys/windows.
Key Algorithms
Relative Path Calculation:
Input: link_path, target_path.
Process: Calculate relative path from dirname(link_path) to target_path.
Output: String used for symlink creation.
Hardlink Detection:
Input: File path.
Process: stat() → Check st_nlink (Unix) or NumberOfLinks (Windows). If > 1, scan directory for other files with same Inode/FileIndex.
Auto-Healing Search:
Input: Broken link string (e.g., /old/path/file.txt).
Process: Extract filename file.txt. Recursively search current project root for file.txt. Compare size/hash. If match, propose new link.
4. Development Milestones
Milestone
Deliverables
Estimated Time
M1: Core CLI
make and ls working on Win/Mac/Linux.
2 Weeks
M2: Conversion
convert and fix commands implemented.
2 Weeks
M3: Manifests
.smartlink.json schema, init, and sync.
3 Weeks
M4: Git Hooks
Auto-sync on git operations, git check for CI.
2 Weeks
M5: Polish
Documentation, Homebrew/Winget packages, CI tests.
2 Weeks
5. Testing Strategy
Unit Tests
Test relative path calculation logic.
Test JSON manifest parsing/validation.
Integration Tests (The "Matrix")
Windows: Test Junctions vs Symlinks vs Hardlinks on NTFS.
macOS: Test Symlinks and clonefile (APFS) on case-sensitive/insensitive volumes.
Linux: Test Bind Mounts (requires sudo handling) and Hardlinks on ext4/Btrfs.
Edge Cases to Handle
Links pointing to non-existent targets.
Circular links (A -> B -> A).
Permissions errors (especially for Linux Bind Mounts).
Cross-drive/partition hardlink attempts (must fail gracefully).
6. Distribution & Marketing
Packaging
Rust: Publish to crates.io.
Binaries: Provide pre-built binaries for:
Windows: Winget, Scoop.
macOS: Homebrew Tap.
Linux: AUR, Debian/RPM repos.
Documentation
README.md: Quick start guide with "Before/After" examples.
Wiki: Deep dive into "Why your symlinks break" and "Hardlinks vs Symlinks."
Target Audience
Frontend Developers (managing node_modules or shared assets).
DevOps Engineers (standardizing build environments).
Data Scientists (managing large datasets via CoW/Links).
7. Immediate Next Steps
Initialize Repo: Set up Rust/Go project structure.
Prototype make: Get basic symlink creation working on your primary OS.
Prototype ls: Get basic file metadata reading working.
Design JSON Schema: Draft the v1 .smartlink.json structure.