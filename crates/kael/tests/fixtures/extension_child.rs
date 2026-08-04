use anyhow::{Context as _, Result};
use kael::{
    EXTENSION_RPC_VERSION, ExtensionHandshake, ExtensionMessage, ExtensionRequest,
    ExtensionResponse, ExtensionTransport,
};

#[cfg(unix)]
fn connect_transport() -> Result<ExtensionTransport> {
    let socket_path =
        std::env::var("KAEL_EXTENSION_SOCKET").context("KAEL_EXTENSION_SOCKET not set")?;
    let transport = kael::UnixDomainSocketTransport::connect(socket_path)?;
    Ok(ExtensionTransport::new(Box::new(transport)))
}

#[cfg(windows)]
fn connect_transport() -> Result<ExtensionTransport> {
    let socket_path =
        std::env::var("KAEL_EXTENSION_SOCKET").context("KAEL_EXTENSION_SOCKET not set")?;
    let pipe_name = socket_path
        .rsplit('\\')
        .next()
        .filter(|name| !name.is_empty())
        .unwrap_or(&socket_path);
    let transport = kael::NamedPipeTransport::client(pipe_name)?;
    Ok(ExtensionTransport::new(Box::new(transport)))
}

fn main() -> Result<()> {
    let mut transport = connect_transport()?;
    match transport.recv_message()? {
        ExtensionMessage::Handshake(ExtensionHandshake::Host { version, .. }) => {
            transport.send_handshake(ExtensionHandshake::Extension {
                version,
                accepted: version == EXTENSION_RPC_VERSION,
            })?;
        }
        other => anyhow::bail!("unexpected first extension message: {:?}", other),
    }

    loop {
        match transport.recv_message()? {
            ExtensionMessage::Rpc(kael::IpcMessage::Request { id, body }) => match body {
                ExtensionRequest::Shutdown | ExtensionRequest::Deactivate => {
                    transport.send_response(id, Ok(ExtensionResponse::Ack))?;
                    break;
                }
                ExtensionRequest::Activate => {
                    transport.send_response(id, Ok(ExtensionResponse::Ack))?;
                }
                ExtensionRequest::GetContributions => {
                    let mut contributions = kael::Contributions::default();
                    contributions.commands.push(kael::ContributedCommand {
                        id: "integration.echo".to_string(),
                        title: "Echo".to_string(),
                        keybinding: None,
                    });
                    transport
                        .send_response(id, Ok(ExtensionResponse::Contributions(contributions)))?;
                }
                ExtensionRequest::ExecuteCommand { command_id, .. } => {
                    if command_id == "integration.crash" {
                        std::process::exit(17);
                    }
                    transport.send_response(id, Ok(ExtensionResponse::Ack))?;
                }
            },
            ExtensionMessage::Handshake(_) | ExtensionMessage::Notification(_) => {}
            ExtensionMessage::Rpc(_) => {}
        }
    }

    Ok(())
}
