// SPDX-License-Identifier: GPL-3.0-only
// The low-level request is intentional: ASHPD 0.13's chooser builder waits for
// the response before returning Request, so it cannot close an active chooser
// when the originating page navigates. Keep the exported parent alive and use
// the portal Request.Close protocol on cancellation.
use anyhow::{bail, Context, Result};
use ashpd::{zbus, zvariant, WindowIdentifier};
use futures::{channel::oneshot, FutureExt, StreamExt};
use serde_json::Value;
use std::collections::HashMap;

pub async fn choose(
    window: &adw::ApplicationWindow,
    event: &Value,
    cancel: oneshot::Receiver<()>,
) -> Result<Vec<String>> {
    eprintln!("file portal: exporting parent");
    let parent = WindowIdentifier::from_native(window)
        .await
        .context("no portal parent window")?;
    eprintln!("file portal: parent exported");
    let connection = zbus::Connection::session().await?;
    let token = format!(
        "bastle_{}_{}",
        std::process::id(),
        event["id"].as_u64().context("request id")?
    );
    let sender = connection
        .unique_name()
        .context("portal connection has no name")?
        .as_str()
        .trim_start_matches(':')
        .replace('.', "_");
    let path = format!("/org/freedesktop/portal/desktop/request/{sender}/{token}");
    let request = zbus::Proxy::new(
        &connection,
        "org.freedesktop.portal.Desktop",
        path.as_str(),
        "org.freedesktop.portal.Request",
    )
    .await?;
    let mut responses = request.receive_signal("Response").await?;
    let chooser = zbus::Proxy::new(
        &connection,
        "org.freedesktop.portal.Desktop",
        "/org/freedesktop/portal/desktop",
        "org.freedesktop.portal.FileChooser",
    )
    .await?;
    let save = event["kind"] == "download-request" || event["mode"] == 3;
    let mut options = HashMap::<&str, zvariant::Value<'_>>::new();
    options.insert("handle_token", token.as_str().into());
    options.insert("modal", true.into());
    if save {
        options.insert(
            "current_name",
            safe_filename(event["name"].as_str().unwrap_or("download")).into(),
        );
    } else {
        options.insert("multiple", (event["mode"] == 1).into());
        options.insert("directory", (event["mode"] == 2).into());
    }
    let filters = file_filters(event);
    if !filters.is_empty() {
        options.insert("filters", zvariant::Value::new(filters));
    }
    let title = if save {
        "Сохранить файл"
    } else {
        "Выбрать файл"
    };
    eprintln!("file portal: requesting chooser");
    let returned: zvariant::OwnedObjectPath = chooser
        .call(
            if save { "SaveFile" } else { "OpenFile" },
            &(parent.to_string(), title, options),
        )
        .await?;
    eprintln!("file portal: chooser opened");
    if returned.as_str() != path {
        bail!("portal returned an unexpected request path");
    }
    let response = responses.next().fuse();
    let cancel = cancel.fuse();
    futures::pin_mut!(response, cancel);
    let message = futures::select! {
        message = response => message.context("portal closed without a response")?,
        _ = cancel => {
            let _: Result<(), zbus::Error> = request.call("Close", &()).await;
            return Ok(Vec::new());
        },
    };
    let (status, mut values): (u32, HashMap<String, zvariant::OwnedValue>) =
        message.body().deserialize()?;
    if status == 1 {
        return Ok(Vec::new());
    }
    if status != 0 {
        bail!("file portal failed ({status})");
    }
    let uris = Vec::<String>::try_from(values.remove("uris").context("portal omitted files")?)?;
    if uris.len() > 256 {
        bail!("too many selected files");
    }
    uris.into_iter()
        .map(|uri| {
            let path = url::Url::parse(&uri)?
                .to_file_path()
                .map_err(|_| anyhow::anyhow!("portal returned a non-local file"))?;
            let path = path.to_str().context("CEF requires a UTF-8 file path")?;
            if path.contains('\0') {
                bail!("invalid file path");
            }
            Ok(path.to_owned())
        })
        .collect()
}

fn safe_filename(name: &str) -> String {
    std::path::Path::new(name)
        .file_name()
        .and_then(|s| s.to_str())
        .filter(|s| !s.is_empty() && !s.contains('\0'))
        .unwrap_or("download")
        .into()
}

fn file_filters(event: &Value) -> Vec<(String, Vec<(u32, String)>)> {
    let Some(filters) = event["filters"].as_array() else {
        return Vec::new();
    };
    let mut rules = Vec::new();
    for filter in filters.iter().take(64).filter_map(Value::as_str) {
        if filter.starts_with('.') && !filter.contains(['/', '\0']) {
            rules.push((0, format!("*{filter}")));
        } else if filter.contains('/') && !filter.contains(['\0', '|', ';']) {
            rules.push((1, filter.to_owned()));
        } else if let Some((_, extensions)) = filter.split_once('|') {
            for extension in extensions.split(';') {
                if extension.starts_with('.') && !extension.contains(['/', '\0']) {
                    rules.push((0, format!("*{extension}")));
                }
            }
        }
    }
    if rules.is_empty() {
        Vec::new()
    } else {
        vec![("Подходящие файлы".into(), rules)]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn filters_preserve_mime_wildcards_and_cef_extensions() {
        let filters = file_filters(
            &serde_json::json!({"filters":["image/*", ".txt", "Documents|.pdf;.odt"]}),
        );
        assert_eq!(
            filters[0].1,
            vec![
                (1, "image/*".into()),
                (0, "*.txt".into()),
                (0, "*.pdf".into()),
                (0, "*.odt".into())
            ]
        );
        assert_eq!(safe_filename("../../document.txt"), "document.txt");
    }
}
