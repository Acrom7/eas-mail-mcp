use std::collections::{BTreeMap, BTreeSet};

/// Policy limits that the client can honestly enforce.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PolicyDecision {
    /// Whether all requirements are supported.
    pub supported: bool,
    /// Human-readable unsupported requirements.
    pub reasons: Vec<String>,
    /// Maximum permitted attachment bytes.
    pub max_attachment_bytes: u64,
    /// Whether attachments are enabled.
    pub attachments_enabled: bool,
    /// Maximum mail body bytes.
    pub body_limit: usize,
    /// Effective mail FilterType.
    pub mail_filter_type: u8,
    /// Effective calendar FilterType.
    pub calendar_filter_type: u8,
}

/// Rejects malformed values and requirements that a per-user mail process cannot enforce.
pub fn evaluate_policy(policy: &BTreeMap<String, String>) -> PolicyDecision {
    let mut reasons = unknown_fields(policy);
    let booleans = boolean_values(policy, &mut reasons);
    validate_device_requirements(policy, &booleans, &mut reasons);
    validate_numeric_fields(policy, &mut reasons);

    let max_attachment =
        integer(policy, "MaxAttachmentSize", 25 * 1024 * 1024, None, false, true, &mut reasons);
    let plain_body =
        integer(policy, "MaxEmailBodyTruncationSize", -1, None, true, false, &mut reasons);
    let html_body =
        integer(policy, "MaxEmailHTMLBodyTruncationSize", -1, None, true, false, &mut reasons);
    validate_body_limit("MaxEmailBodyTruncationSize", plain_body, &mut reasons);
    validate_body_limit("MaxEmailHTMLBodyTruncationSize", html_body, &mut reasons);
    let selected_body = if booleans.get("AllowHTMLEmail").copied().unwrap_or(1) == 1 {
        html_body
    } else {
        plain_body
    };
    let mail_filter = integer(
        policy,
        "MaxEmailAgeFilter",
        0,
        Some(&[0, 1, 2, 3, 4, 5]),
        false,
        true,
        &mut reasons,
    );
    let calendar_filter = integer(
        policy,
        "MaxCalendarAgeFilter",
        0,
        Some(&[0, 4, 5, 6, 7]),
        false,
        true,
        &mut reasons,
    );
    let body_limit = if selected_body < 0 { 50_000 } else { selected_body };

    PolicyDecision {
        supported: reasons.is_empty(),
        reasons,
        max_attachment_bytes: u64::try_from(max_attachment)
            .unwrap_or(25 * 1024 * 1024)
            .min(25 * 1024 * 1024),
        attachments_enabled: booleans.get("AttachmentsEnabled").copied().unwrap_or(1) == 1,
        body_limit: usize::try_from(body_limit).unwrap_or(50_000).min(50_000),
        mail_filter_type: if mail_filter == 0 { 5 } else { mail_filter.min(5) as u8 },
        calendar_filter_type: if calendar_filter == 0 { 6 } else { calendar_filter.min(6) as u8 },
    }
}

fn unknown_fields(policy: &BTreeMap<String, String>) -> Vec<String> {
    let known = known_fields();
    policy
        .keys()
        .filter(|name| !known.contains(name.as_str()))
        .map(|name| format!("unsupported policy field: {name}"))
        .collect()
}

fn boolean_values(
    policy: &BTreeMap<String, String>,
    reasons: &mut Vec<String>,
) -> BTreeMap<&'static str, i64> {
    boolean_fields()
        .into_iter()
        .filter(|name| policy.contains_key(*name))
        .map(|name| {
            let default = i64::from(name.starts_with("Allow") || name == "AttachmentsEnabled");
            let value = integer(policy, name, default, Some(&[0, 1]), false, false, reasons);
            (name, value)
        })
        .collect()
}

fn validate_device_requirements(
    policy: &BTreeMap<String, String>,
    booleans: &BTreeMap<&str, i64>,
    reasons: &mut Vec<String>,
) {
    for (name, description) in strict_requirements() {
        if booleans.get(name).copied().unwrap_or(0) != 0 {
            reasons.push(format!("unsupported policy requirement: {description}"));
        }
    }
    for (name, description) in restricted_features() {
        if policy.contains_key(name) && booleans.get(name).copied() == Some(0) {
            reasons.push(format!("unsupported device-wide policy: {description}"));
        }
    }
    if policy.contains_key("AllowBluetooth")
        && integer(policy, "AllowBluetooth", 2, Some(&[0, 1, 2]), false, true, reasons) < 2
    {
        reasons.push("unsupported device-wide policy: Bluetooth restriction".into());
    }
    for (name, description) in application_lists() {
        if integer(policy, name, 0, None, false, true, reasons) > 0 {
            reasons.push(format!("unsupported device-wide policy: {description}"));
        }
    }
}

fn validate_numeric_fields(policy: &BTreeMap<String, String>, reasons: &mut Vec<String>) {
    for name in [
        "DevicePasswordExpiration",
        "DevicePasswordHistory",
        "MaxDevicePasswordFailedAttempts",
        "MaxInactivityTimeDeviceLock",
        "MinDevicePasswordComplexCharacters",
        "MinDevicePasswordLength",
        "RequireEncryptionSMIMEAlgorithm",
        "RequireSignedSMIMEAlgorithm",
    ] {
        let _ = integer(policy, name, 0, None, false, true, reasons);
    }
    let _ = integer(
        policy,
        "AllowSMIMEEncryptionAlgorithmNegotiation",
        2,
        Some(&[0, 1, 2]),
        false,
        true,
        reasons,
    );
}

fn validate_body_limit(name: &str, value: i64, reasons: &mut Vec<String>) {
    if value < -1 {
        reasons.push(format!("invalid policy value: {name}"));
    }
}

fn integer(
    policy: &BTreeMap<String, String>,
    name: &str,
    default: i64,
    allowed: Option<&[i64]>,
    signed: bool,
    empty_allowed: bool,
    reasons: &mut Vec<String>,
) -> i64 {
    let Some(raw) = policy.get(name) else {
        return default;
    };
    if raw.is_empty() {
        if !empty_allowed {
            reasons.push(format!("invalid policy value: {name}"));
        }
        return default;
    }
    let Ok(value) = raw.parse::<i64>() else {
        reasons.push(format!("invalid policy value: {name}"));
        return default;
    };
    if (!signed && value < 0) || allowed.is_some_and(|values| !values.contains(&value)) {
        reasons.push(format!("invalid policy value: {name}"));
        return default;
    }
    value
}

fn known_fields() -> BTreeSet<&'static str> {
    [
        "AllowBluetooth",
        "AllowBrowser",
        "AllowCamera",
        "AllowConsumerEmail",
        "AllowDesktopSync",
        "AllowHTMLEmail",
        "AllowInternetSharing",
        "AllowIrDA",
        "AllowPOPIMAPEmail",
        "AllowRemoteDesktop",
        "AllowSimpleDevicePassword",
        "AllowSMIMEEncryptionAlgorithmNegotiation",
        "AllowSMIMESoftCerts",
        "AllowStorageCard",
        "AllowTextMessaging",
        "AllowUnsignedApplications",
        "AllowUnsignedInstallationPackages",
        "AllowWiFi",
        "AlphanumericDevicePasswordRequired",
        "ApprovedApplicationList",
        "AttachmentsEnabled",
        "DevicePasswordEnabled",
        "DevicePasswordExpiration",
        "DevicePasswordHistory",
        "MaxAttachmentSize",
        "MaxCalendarAgeFilter",
        "MaxDevicePasswordFailedAttempts",
        "MaxEmailAgeFilter",
        "MaxEmailBodyTruncationSize",
        "MaxEmailHTMLBodyTruncationSize",
        "MaxInactivityTimeDeviceLock",
        "MinDevicePasswordComplexCharacters",
        "MinDevicePasswordLength",
        "PasswordRecoveryEnabled",
        "RequireDeviceEncryption",
        "RequireEncryptedSMIMEMessages",
        "RequireEncryptionSMIMEAlgorithm",
        "RequireManualSyncWhenRoaming",
        "RequireSignedSMIMEAlgorithm",
        "RequireSignedSMIMEMessages",
        "RequireStorageCardEncryption",
        "UnapprovedInROMApplicationList",
    ]
    .into_iter()
    .collect()
}

fn boolean_fields() -> [&'static str; 25] {
    [
        "AllowBrowser",
        "AllowCamera",
        "AllowConsumerEmail",
        "AllowDesktopSync",
        "AllowHTMLEmail",
        "AllowInternetSharing",
        "AllowIrDA",
        "AllowPOPIMAPEmail",
        "AllowRemoteDesktop",
        "AllowSimpleDevicePassword",
        "AllowSMIMESoftCerts",
        "AllowStorageCard",
        "AllowTextMessaging",
        "AllowUnsignedApplications",
        "AllowUnsignedInstallationPackages",
        "AllowWiFi",
        "AlphanumericDevicePasswordRequired",
        "AttachmentsEnabled",
        "DevicePasswordEnabled",
        "PasswordRecoveryEnabled",
        "RequireDeviceEncryption",
        "RequireEncryptedSMIMEMessages",
        "RequireManualSyncWhenRoaming",
        "RequireSignedSMIMEMessages",
        "RequireStorageCardEncryption",
    ]
}

fn strict_requirements() -> [(&'static str, &'static str); 6] {
    [
        ("DevicePasswordEnabled", "device password enforcement"),
        ("RequireDeviceEncryption", "device-wide encryption enforcement"),
        ("RequireStorageCardEncryption", "storage-card encryption"),
        ("RequireSignedSMIMEMessages", "mandatory S/MIME signatures"),
        ("RequireEncryptedSMIMEMessages", "mandatory S/MIME encryption"),
        ("RequireManualSyncWhenRoaming", "manual synchronization while roaming"),
    ]
}

fn restricted_features() -> [(&'static str, &'static str); 13] {
    [
        ("AllowBrowser", "browser restriction"),
        ("AllowCamera", "camera restriction"),
        ("AllowConsumerEmail", "consumer-email restriction"),
        ("AllowDesktopSync", "desktop-sync restriction"),
        ("AllowInternetSharing", "internet-sharing restriction"),
        ("AllowIrDA", "IrDA restriction"),
        ("AllowPOPIMAPEmail", "POP/IMAP restriction"),
        ("AllowRemoteDesktop", "remote-desktop restriction"),
        ("AllowStorageCard", "storage-card restriction"),
        ("AllowTextMessaging", "text-messaging restriction"),
        ("AllowUnsignedApplications", "unsigned-application restriction"),
        ("AllowUnsignedInstallationPackages", "unsigned-package restriction"),
        ("AllowWiFi", "Wi-Fi restriction"),
    ]
}

fn application_lists() -> [(&'static str, &'static str); 2] {
    [
        ("ApprovedApplicationList", "approved-application allowlist"),
        ("UnapprovedInROMApplicationList", "blocked built-in applications"),
    ]
}
