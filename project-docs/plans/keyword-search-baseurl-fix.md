# Keyword Search Baseurl Fix

Address user-reported issue with baseurl validation in keyword search functionality.

## Issue Description
User reports encountering errors related to baseurl validation when attempting to use the keyword download feature. The error occurs during the validation phase before downloading begins.

## Root Cause Analysis
The issue was in the `validate_http_url` function in `src-tauri/src/lib.rs`. The function had an arbitrary minimum length check of 12 characters:
```rust
if trimmed.len() < 12 {
    return Err("URL looks too short to be valid.".to_string());
}
```
This was incorrectly rejecting valid short URLs like:
- "http://a.co" (10 characters)
- "http://youtu.be/test" would work, but many legitimate short URLs were being blocked

## Fix Applied
Updated the `validate_http_url` function to:
1. Keep the essential protocol check (http:// or https://)
2. Replace the arbitrary 12-character limit with a reasonable 8-character minimum
   - This allows for the shortest valid URLs like "http://a.co" (10 chars) and "https://a.co" (11 chars)
3. Improved the error message to be more helpful and descriptive

### Changes Made:
```rust
fn validate_http_url(url: &str) -> Result<(), String> {
    let trimmed = url.trim();
    if !trimmed.starts_with("http://") && !trimmed.starts_with("https://") {
        return Err("URL must start with http:// or https://".to_string());
    }
    // Must have at least "http://x" or "https://x" (8 characters minimum)
    if trimmed.len() < 8 {
        return Err("URL is too short. Please provide a valid URL including http:// or https:// prefix.".to_string());
    }
    Ok(())
}
```

## Verification
The fix allows:
- Standard YouTube URLs: https://www.youtube.com/watch?v=...
- Short YouTube URLs: https://youtu.be/...
- Other valid web URLs with short domains
- Still rejects invalid URLs missing proper protocol

Error messages are now clearer and more actionable for users.

## Status
COMPLETED: Issue identified and resolved