use std::path::Path;

use serde::{Deserialize, Serialize};
use tracing::info;

use crate::errors::AppError;

/// License tier — Free or Pro.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LicenseTier {
    Free,
    Pro,
}

/// Response returned to the frontend after validation.
#[derive(Debug, Serialize)]
pub struct LicenseResponse {
    pub valid: bool,
    pub tier: LicenseTier,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

/// Credentials persisted to disk when "remember me" is checked.
#[derive(Debug, Serialize, Deserialize)]
pub struct SavedCredentials {
    pub username: String,
    pub license_key: String,
}

const VALIDATION_API: &str = "https://lb-quant.com/api/license/validate";

/// API response from the validation endpoint.
#[derive(Debug, Deserialize)]
struct ApiValidationResponse {
    valid: bool,
    tier: String,
    message: Option<String>,
}

/// Validate a license key against the LBQuant web API.
///
/// All users (free and pro) must provide a valid license key.
/// Calls the remote validation API for every request.
/// On network error → falls back to Free tier with error message.
pub async fn validate_license(username: &str, license_key: &str) -> LicenseResponse {
    let username = username.trim();
    let key = license_key.trim();

    if username.is_empty() {
        return LicenseResponse {
            valid: false,
            tier: LicenseTier::Free,
            message: Some("Username is required".to_string()),
        };
    }

    if key.is_empty() {
        return LicenseResponse {
            valid: false,
            tier: LicenseTier::Free,
            message: Some("License key is required. Create a free account at lb-quant.com/register".to_string()),
        };
    }

    // Call the remote validation API
    info!("Validating license key for user '{}'", username);
    let client = reqwest::Client::new();
    let result = client
        .post(VALIDATION_API)
        .json(&serde_json::json!({
            "username": username,
            "license_key": key,
        }))
        .timeout(std::time::Duration::from_secs(10))
        .send()
        .await;

    match result {
        Ok(resp) if resp.status().is_success() => {
            match resp.json::<ApiValidationResponse>().await {
                Ok(api_resp) => {
                    let tier = if api_resp.tier == "pro" {
                        LicenseTier::Pro
                    } else {
                        LicenseTier::Free
                    };
                    info!(
                        "User '{}' validation: valid={}, tier={:?}",
                        username, api_resp.valid, tier
                    );
                    LicenseResponse {
                        valid: api_resp.valid,
                        tier,
                        message: api_resp.message,
                    }
                }
                Err(e) => {
                    tracing::error!("Failed to parse validation response: {}", e);
                    LicenseResponse {
                        valid: false,
                        tier: LicenseTier::Free,
                        message: Some("Invalid server response".to_string()),
                    }
                }
            }
        }
        Ok(resp) => {
            tracing::error!("Validation API returned status {}", resp.status());
            LicenseResponse {
                valid: false,
                tier: LicenseTier::Free,
                message: Some("License validation failed".to_string()),
            }
        }
        Err(e) => {
            tracing::error!("Network error during license validation: {}", e);
            LicenseResponse {
                valid: false,
                tier: LicenseTier::Free,
                message: Some(
                    "Could not validate license. Check your internet connection.".to_string(),
                ),
            }
        }
    }
}

/// Save credentials to `data/license.json`.
pub fn save_credentials(data_dir: &Path, username: &str, license_key: &str) -> Result<(), AppError> {
    let creds = SavedCredentials {
        username: username.to_string(),
        license_key: license_key.to_string(),
    };
    let json = serde_json::to_string_pretty(&creds)?;
    let path = data_dir.join("license.json");
    std::fs::write(&path, json)?;
    info!("Saved credentials to {}", path.display());
    Ok(())
}

/// Load saved credentials from `data/license.json`.
pub fn load_credentials(data_dir: &Path) -> Option<SavedCredentials> {
    let path = data_dir.join("license.json");
    let content = std::fs::read_to_string(&path).ok()?;
    serde_json::from_str(&content).ok()
}

/// Remove saved credentials file.
pub fn clear_credentials(data_dir: &Path) -> Result<(), AppError> {
    let path = data_dir.join("license.json");
    if path.exists() {
        std::fs::remove_file(&path)?;
        info!("Cleared saved credentials");
    }
    Ok(())
}
