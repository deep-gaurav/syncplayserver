use std::{collections::HashMap, sync::Arc};

use async_graphql::*;
use dashmap::DashMap;
use serde::Serialize;
use tokio::sync::{mpsc::Sender, RwLock};

#[derive(Default)]
pub struct Storage {
    pub private_rooms: Arc<DashMap<String, Room>>,
}

#[derive(Serialize, SimpleObject, Clone)]
#[graphql(complex)]
pub struct Room {
    pub id: String,
    pub users: Vec<LobbyPlayer>,
}

#[ComplexObject]
impl Room {
    pub async fn players(&self) -> &[LobbyPlayer] {
        &self.users
    }
}

impl Room {
    pub fn new(id: String, player: Player) -> Self {
        Self { id, users: vec![] }
    }
}

impl Room {
    pub fn add_player(&mut self, player: Player) -> Result<(), anyhow::Error> {
        if self.users.iter().any(|p| p.player.id == player.id) {
            Ok(())
        } else {
            self.users.push(LobbyPlayer {
                player,
                send_channel: None,
                state: UserState::NotReady(NotReadyData { empty: 0 }),
            });
            Ok(())
        }
    }

    pub fn is_empty(&self) -> bool {
        self.users.is_empty() || self.users.iter().all(|user| user.has_channel())
    }

    pub fn set_player_channel(
        &mut self,
        player_id: String,
        channel: Sender<ServerResponse>,
    ) -> Result<(), anyhow::Error> {
        let pl = self.users.iter_mut().find(|p| p.player.id == player_id);
        if let Some(pl) = pl {
            pl.send_channel = Some(channel);
            Ok(())
        } else {
            Err(anyhow::anyhow!("Player does not exist"))
        }
    }

    pub fn get_player(&self, player_id: &str) -> Option<&LobbyPlayer> {
        self.users
            .iter()
            .find(|p| p.player.id == player_id)
            .map(|lp| lp)
    }

    pub fn get_player_mut(&mut self, player_id: &str) -> Option<&mut LobbyPlayer> {
        self.users
            .iter_mut()
            .find(|p| p.player.id == player_id)
            .map(|lp| lp)
    }

    pub fn disconnect_player(&mut self, player_id: &str) -> Result<(), anyhow::Error> {
        log::info!("Removing player {}", player_id);
        if let Some(player) = self.users.iter_mut().find(|p| p.player.id == player_id) {
            player.send_channel = None;
            Ok(())
        } else {
            Err(anyhow::anyhow!("Player does not exist"))
        }
    }

    pub fn remove_player(&mut self, player_id: &str) -> Result<Player, anyhow::Error> {
        log::info!("Removing player {}", player_id);
        let p_index = self
            .users
            .iter()
            .position(|p| p.player.id == player_id)
            .ok_or_else(|| anyhow::anyhow!("Player doesnt exist"))?;
        let player = self.users.remove(p_index);
        Ok(player.player)
    }
}

#[derive(Debug, SimpleObject, Serialize, Clone)]
#[graphql(complex)]
pub struct LobbyPlayer {
    pub player: Player,

    #[serde(skip_serializing)]
    #[graphql(skip)]
    pub send_channel: Option<Sender<ServerResponse>>,

    pub state: UserState,
}

#[ComplexObject]
impl LobbyPlayer {
    pub async fn is_connected<'ctx>(
        &self,
        _ctx: &Context<'_>,
    ) -> Result<bool, async_graphql::Error> {
        Ok(self.send_channel.is_some())
    }
}

#[derive(Debug, Union, Serialize, Clone)]
pub enum UserState {
    NotReady(NotReadyData),
    Ready(ReadyData),
}

impl UserState {
    pub fn as_ready(&self) -> Option<&ReadyData> {
        if let Self::Ready(v) = self {
            Some(v)
        } else {
            None
        }
    }
}

#[derive(Debug, SimpleObject, Serialize, Clone)]
pub struct NotReadyData {
    empty: u32,
}

#[derive(Debug, SimpleObject, Serialize, Clone)]
pub struct ReadyData {
    pub playing: bool,
    pub position_secs: u64,
}

impl Room {
    pub fn get_players(&self) -> &[LobbyPlayer] {
        &self.users
    }
    pub async fn broadcast(&self, message: ServerResponse) {
        let futures = self.get_players().iter().map(|f| f.send(message.clone()));
        futures::future::join_all(futures).await;
    }
}

impl LobbyPlayer {
    fn get_channel(&self) -> &Option<Sender<ServerResponse>> {
        &self.send_channel
    }

    async fn send(&self, message: ServerResponse) {
        match self.get_channel() {
            Some(channel) => match channel.send(message).await {
                Ok(_) => {}
                Err(_er) => {
                    log::warn!("ERROR SENDING ")
                }
            },
            None => {}
        }
    }

    fn has_channel(&self) -> bool {
        self.get_channel().is_some()
    }
}

#[derive(SimpleObject, Serialize, Clone, Debug)]
pub struct Player {
    pub id: String,
    pub name: String,
}

#[derive(SimpleObject, Serialize, Clone)]
pub struct PlayerJoined {
    pub player: Player,
    pub room: Room,
}

#[derive(SimpleObject, Serialize, Clone)]
pub struct PlayerLeft {
    pub player: Player,
    pub room: Room,
}

#[derive(SimpleObject, Serialize, Clone)]
pub struct PlayerConnected {
    pub player: Player,
    pub room: Room,
}

#[derive(SimpleObject, Serialize, Clone)]
pub struct PlayerRemoved {
    pub player: Player,
    pub room: Room,
}

#[derive(SimpleObject, Serialize, Clone)]
pub struct ChatMessage {
    pub player: Player,
    pub message: String,
}

#[derive(Serialize, Union, Clone)]
pub enum ServerResponse {
    PlayerJoined(PlayerJoined),
    PlayerConnected(PlayerConnected),
    PlayerLeft(PlayerLeft),
    PlayerRemoved(PlayerRemoved),

    StatusUpdate(ReadyData),

    ChatMessage(ChatMessage),
}
