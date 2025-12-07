# Security Considerations

## Overview

Memoire captures sensitive screen content and stores it locally. This document covers security measures implemented and recommendations for future hardening.

## Current Security Measures

### Memory Safety

#### Bounds-Checked Pixel Copy (screen.rs)

The DXGI frame capture includes comprehensive bounds validation:

```rust
// Location: memoire-capture/src/screen.rs:162-218

// 1. Validate row pitch
let min_row_pitch = width.checked_mul(4).ok_or_else(|| {
    CaptureError::FrameAcquisition("width overflow".into())
})?;

if row_pitch < min_row_pitch {
    return Err(CaptureError::FrameAcquisition("invalid row_pitch".into()));
}

// 2. Validate total buffer size (overflow check)
let total_size = width.checked_mul(height)
    .and_then(|wh| wh.checked_mul(4))
    .ok_or_else(|| CaptureError::FrameAcquisition("buffer overflow".into()))?;

// 3. Null pointer validation
if mapped.pData.is_null() {
    return Err(CaptureError::FrameAcquisition("null pData".into()));
}

// 4. Row offset overflow protection
for y in 0..height {
    let row_offset = y.checked_mul(row_pitch).ok_or_else(|| {
        CaptureError::FrameAcquisition("row offset overflow".into())
    })?;
    // Safe pixel access...
}
```

#### Panic Prevention (recorder.rs)

Replaced panic-inducing `unwrap()` with error recovery:

```rust
// Location: memoire-capture/src/recorder.rs:81-90

let chunk_id = match self.current_chunk_id {
    Some(id) => id,
    None => {
        error!("chunk_id unexpectedly None - attempting recovery");
        self.start_new_chunk(db)?;
        self.current_chunk_id
            .ok_or_else(|| anyhow::anyhow!("failed to initialize chunk_id"))?
    }
};
```

### Path Security

#### Monitor Name Sanitization (recorder.rs:319-366)

Monitor names are sanitized before use as directory names:

```rust
const WINDOWS_RESERVED_NAMES: &[&str] = &[
    "CON", "PRN", "AUX", "NUL",
    "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8", "COM9",
    "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

fn sanitize_monitor_name(name: &str) -> String {
    // Step 1: Replace invalid filesystem characters
    // \ / : * ? " < > |
    // Control characters (0x00-0x1F)

    // Step 2: Remove path traversal sequences
    // ".." -> "_"

    // Step 3: Trim whitespace and special chars

    // Step 4: Block Windows reserved names
    // Prefix with underscore: CON -> _CON

    // Step 5: Truncate to 100 characters

    // Step 6: Fallback to "monitor" if empty
}
```

**Protected against:**
- Path traversal: `../../../etc/passwd` → `etc_passwd`
- Windows reserved names: `CON` → `_CON`
- Control characters: `file\x00name` → `file_name`
- Long filenames: Truncated to 100 chars

### FFmpeg Command Safety

Rust's `std::process::Command` properly escapes arguments:

```rust
// Safe - arguments are passed as separate strings, not shell-interpolated
let mut cmd = Command::new("ffmpeg");
cmd.arg("-y")
   .arg("-s").arg(format!("{}x{}", width, height))  // Safe
   .arg("-r").arg(self.config.fps.to_string())       // Safe
   .arg(&output_path);                               // Safe
```

**NOT vulnerable to command injection** because:
- No shell interpretation (`Command` bypasses shell)
- Arguments passed as array, not concatenated string
- User input (monitor names) sanitized before use in paths

## Data Storage Security

### Local Storage

All data is stored locally in `%LOCALAPPDATA%\Memoire\`:

```
Memoire/
├── memoire.db       # SQLite database
└── videos/          # MP4 video chunks
    └── {monitor}/
        └── {date}/
```

### Database

- SQLite file-based storage
- No network exposure
- WAL mode for concurrent access

### Video Files

- MP4 chunks with H.264 encoding
- No encryption (future consideration)
- Organized by monitor/date

## Threat Model

### In Scope

| Threat | Status | Notes |
|--------|--------|-------|
| Memory corruption | Mitigated | Bounds checking, Rust safety |
| Path traversal | Mitigated | Input sanitization |
| Command injection | Not vulnerable | Rust Command struct |
| Panic/crash | Mitigated | Error handling |

### Out of Scope (Future Work)

| Threat | Status | Recommendation |
|--------|--------|----------------|
| Unauthorized access | Not addressed | File permissions, encryption |
| Screen content exposure | Not addressed | At-rest encryption |
| Database tampering | Not addressed | Integrity checks |
| Process injection | Not addressed | Code signing |

## Recommendations

### Short-Term

1. **File Permissions**: Set restrictive ACLs on data directory
2. **Config Encryption**: Encrypt sensitive configuration values
3. **Audit Logging**: Log access to captured data

### Long-Term

1. **At-Rest Encryption**: Encrypt video files and database
2. **Memory Protection**: Clear sensitive data after use
3. **Secure Deletion**: Overwrite files before deletion
4. **Access Control**: Authentication for API access
5. **Code Signing**: Sign executables

## Security Testing Performed

| Test | Result |
|------|--------|
| Path traversal fuzzing | Passed |
| Integer overflow | Passed (checked arithmetic) |
| Null pointer handling | Passed |
| Reserved name handling | Passed |
| Command injection | Not vulnerable |

## Reporting Security Issues

Security vulnerabilities should be reported privately. Do not create public issues for security concerns.

## Version History

| Version | Changes |
|---------|---------|
| 0.1.0 | Initial security review, bounds checking, path sanitization |
