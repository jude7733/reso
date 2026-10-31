use crate::error::{ResoError, Result};
use std::collections::HashMap;
use zbus::zvariant::Value;

pub async fn notify(summary: &str, body: &str) -> Result<()> {
    let connection = zbus::Connection::session().await.map_err(|error| {
        ResoError::Custom(format!("Desktop notifications unavailable: {error}"))
    })?;
    let proxy = zbus::Proxy::new(
        &connection,
        "org.freedesktop.Notifications",
        "/org/freedesktop/Notifications",
        "org.freedesktop.Notifications",
    )
    .await
    .map_err(|error| ResoError::Custom(format!("Desktop notifications unavailable: {error}")))?;
    let actions: [&str; 0] = [];
    let hints: HashMap<&str, Value<'_>> = HashMap::new();
    proxy
        .call_method(
            "Notify",
            &("reso", 0u32, "", summary, body, actions, hints, 5000i32),
        )
        .await
        .map_err(|error| ResoError::Custom(format!("Desktop notification failed: {error}")))?;
    Ok(())
}
