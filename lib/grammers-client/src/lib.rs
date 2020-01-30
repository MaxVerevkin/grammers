use std::io::{self, Result};

use grammers_mtsender::MTSender;
use grammers_session::Session;
use grammers_tl_types::{self as tl, Serializable, RPC};

// TODO handle PhoneMigrate
const DC_4_ADDRESS: &'static str = "149.154.167.91:443";

/// A client capable of connecting to Telegram and invoking requests.
pub struct Client {
    sender: MTSender,
}

/// Implementors of this trait have a way to turn themselves into the
/// desired input parameter.
pub trait IntoInput<T> {
    fn convert(&self, client: &mut Client) -> Result<T>;
}

impl IntoInput<tl::enums::InputPeer> for tl::types::User {
    fn convert(&self, _client: &mut Client) -> Result<tl::enums::InputPeer> {
        if let Some(access_hash) = self.access_hash {
            Ok(tl::enums::InputPeer::InputPeerUser(
                tl::types::InputPeerUser {
                    user_id: self.id,
                    access_hash: access_hash,
                },
            ))
        } else {
            Err(io::Error::new(
                io::ErrorKind::NotFound,
                "user is missing access_hash",
            ))
        }
    }
}

impl IntoInput<tl::enums::InputPeer> for &str {
    fn convert(&self, client: &mut Client) -> Result<tl::enums::InputPeer> {
        if let Some(user) = client.resolve_username(self)? {
            user.convert(client)
        } else {
            Err(io::Error::new(
                io::ErrorKind::NotFound,
                "no user has that username",
            ))
        }
    }
}

impl Client {
    /// Returns a new client instance connected to Telegram and returns it.
    ///
    /// This method will generate a new authorization key and connect to a
    /// default datacenter. To prevent logging in every single time, use
    /// [`with_session`] instead, which will reuse a previous session.
    pub fn new() -> Result<Self> {
        let mut sender = MTSender::connect(DC_4_ADDRESS)?;
        sender.generate_auth_key()?;
        Self::with_sender(sender)
    }

    /// Configures a new client instance from an existing session and returns
    /// it.
    pub fn with_session(mut session: Box<dyn Session>) -> Result<Self> {
        // TODO this doesn't look clean, and configuring the authkey
        //      on the sender this way also seems a bit weird.
        let auth_key;
        let server_id;
        let server_address;
        if let Some((dc_id, dc_addr)) = session.get_user_datacenter() {
            server_id = dc_id;
            server_address = dc_addr;
            auth_key = session.get_auth_key_data(dc_id);
        } else {
            server_id = 4;
            server_address = DC_4_ADDRESS.parse().unwrap();
            auth_key = None;
            session.set_user_datacenter(server_id, &server_address);
            session.save()?;
        }

        let mut sender = MTSender::connect(server_address)?;
        if let Some(auth_key) = auth_key {
            sender.set_auth_key(auth_key);
        } else {
            let auth_key = sender.generate_auth_key()?;
            session.set_auth_key_data(server_id, &auth_key.to_bytes());
            session.save()?;
        }

        Self::with_sender(sender)
    }

    /// Creates a client instance with a sender
    fn with_sender(sender: MTSender) -> Result<Self> {
        let mut client = Client { sender };
        client.init_connection()?;
        Ok(client)
    }

    /// Signs in to the bot account associated with this token.
    pub fn bot_sign_in(&mut self, token: &str, api_id: i32, api_hash: &str) -> Result<()> {
        self.invoke(&tl::functions::auth::ImportBotAuthorization {
            flags: 0,
            api_id,
            api_hash: api_hash.to_string(),
            bot_auth_token: token.to_string(),
        })?;

        Ok(())
    }

    /// Resolves a username into the user that owns it, if any.
    pub fn resolve_username(&mut self, username: &str) -> Result<Option<tl::types::User>> {
        let tl::types::contacts::ResolvedPeer { peer, users, .. } =
            match self.invoke(&tl::functions::contacts::ResolveUsername {
                username: username.into(),
            })? {
                tl::enums::contacts::ResolvedPeer::ResolvedPeer(x) => x,
            };

        match peer {
            tl::enums::Peer::PeerUser(tl::types::PeerUser { user_id }) => {
                return Ok(users
                    .into_iter()
                    .filter_map(|user| match user {
                        tl::enums::User::User(user) => {
                            if user.id == user_id {
                                Some(user)
                            } else {
                                None
                            }
                        }
                        tl::enums::User::UserEmpty(_) => None,
                    })
                    .next());
            }
            tl::enums::Peer::PeerChat(_) => {}
            tl::enums::Peer::PeerChannel(_) => {}
        }

        Ok(None)
    }

    /// Sends a text message to the desired chat.
    pub fn send_message<C: IntoInput<tl::enums::InputPeer>>(
        &mut self,
        chat: C,
        message: &str,
    ) -> Result<()> {
        let chat = chat.convert(self)?;
        self.invoke(&tl::functions::messages::SendMessage {
            no_webpage: false,
            silent: false,
            background: false,
            clear_draft: false,
            peer: chat,
            reply_to_msg_id: None,
            message: message.into(),
            random_id: 1337,
            reply_markup: None,
            entities: None,
            schedule_date: None,
        })?;
        Ok(())
    }

    /// Initializes the connection with Telegram. If this is never done on
    /// a fresh session, then Telegram won't know which layer to use and a
    /// very old one will be used (which we will fail to understand).
    fn init_connection(&mut self) -> Result<()> {
        // TODO store config
        self.invoke(&tl::functions::InvokeWithLayer {
            layer: tl::LAYER,
            query: tl::functions::InitConnection {
                api_id: 6,
                device_model: "Linux".into(),
                system_version: "4.15.0-74-generic".into(),
                app_version: "0.1.0".into(),
                system_lang_code: "en".into(),
                lang_pack: "".into(),
                lang_code: "en".into(),
                proxy: None,
                query: tl::functions::help::GetConfig {}.to_bytes(),
            }
            .to_bytes(),
        })?;
        Ok(())
    }

    /// Invokes a raw request, and returns its result.
    pub fn invoke<R: RPC>(&mut self, request: &R) -> Result<R::Return> {
        self.sender.invoke(request)
    }
}
