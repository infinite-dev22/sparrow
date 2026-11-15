//! Callback transport for the native Dart debugger and compatibility RPC replies.
use crossbeam_channel::Sender;
use dyn_clone::DynClone;
use sparrow_rpc::plugin::PluginId;
pub enum ResponseHandler<Resp, Error> {
    Chan(Sender<Result<Resp, Error>>),
    Callback(Box<dyn RpcCallback<Resp, Error>>),
}

impl<Resp, Error> ResponseHandler<Resp, Error> {
    pub fn invoke(self, result: Result<Resp, Error>) {
        match self {
            ResponseHandler::Chan(tx) => {
                if let Err(err) = tx.send(result) {
                    tracing::error!("{:?}", err);
                }
            }
            ResponseHandler::Callback(f) => f.call(result),
        }
    }
}

pub trait CloneableCallback<Resp, Error>:
    FnOnce(PluginId, Result<Resp, Error>) + Send + DynClone
{
}

impl<Resp, Error, F: Send + FnOnce(PluginId, Result<Resp, Error>) + DynClone>
    CloneableCallback<Resp, Error> for F
{
}

pub trait RpcCallback<Resp, Error>: Send {
    fn call(self: Box<Self>, result: Result<Resp, Error>);
}

impl<Resp, Error, F: Send + FnOnce(Result<Resp, Error>)> RpcCallback<Resp, Error>
    for F
{
    fn call(self: Box<F>, result: Result<Resp, Error>) {
        (*self)(result)
    }
}

/// Read a Dart DAP Content-Length frame without the retired LSP/plugin host.
pub fn read_message(reader: &mut impl std::io::BufRead) -> anyhow::Result<String> {
    let mut length = None;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 {
            anyhow::bail!("Debugger stream ended before its headers");
        }
        if line.trim().is_empty() {
            break;
        }
        if let Some((name, value)) = line.split_once(':') {
            if name.eq_ignore_ascii_case("Content-Length") {
                length = Some(value.trim().parse::<usize>()?);
            }
        }
    }
    let length =
        length.ok_or_else(|| anyhow::anyhow!("Missing debugger Content-Length"))?;
    if length > 16 * 1024 * 1024 {
        anyhow::bail!("Debugger frame exceeds 16 MiB");
    }
    let mut body = vec![0; length];
    reader.read_exact(&mut body)?;
    Ok(String::from_utf8(body)?)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn dap_frames_keep_utf8_and_consecutive_messages() {
        let text = "{\"text\":\"λ\"}";
        let input = format!(
            "Content-Length: {}\r\n\r\n{}Content-Length: 2\r\n\r\n{{}}",
            text.len(),
            text
        );
        let mut reader = std::io::Cursor::new(input);
        assert_eq!(read_message(&mut reader).unwrap(), text);
        assert_eq!(read_message(&mut reader).unwrap(), "{}");
    }
    #[test]
    fn rejects_missing_or_truncated_frames() {
        assert!(read_message(&mut std::io::Cursor::new("\r\n")).is_err());
        assert!(
            read_message(&mut std::io::Cursor::new("Content-Length: 10\r\n\r\n{}"))
                .is_err()
        );
    }
}
