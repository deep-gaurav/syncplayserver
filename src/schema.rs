use std::sync::Arc;

use async_graphql::{Context, Object, Subscription};
use dashmap::DashMap;
use futures::Stream;
use tokio::sync::mpsc::channel;
use tokio::sync::mpsc::Receiver;

use crate::data::ChatMessage;
use crate::data::Player;
use crate::data::PlayerConnected;
use crate::data::PlayerJoined;
use crate::data::PlayerLeft;
use crate::data::PlayerRemoved;
use crate::data::ReadyData;
use crate::data::Room;
use crate::data::ServerResponse;
use crate::data::Storage;
use crate::data::UserState;
use crate::utils::generate_rand_string;

pub struct QueryRoot;

#[Object]
impl QueryRoot {
    pub async fn hello(&self) -> String {
        "hello world".to_string()
    }

    pub async fn ping(&self) -> String {
        "pong".into()
    }

    pub async fn status_update<'ctx>(
        &self,
        ctx: &Context<'_>,
        user_id: String,
        room_id: String,
        is_playing: bool,
        position_secs: u64,
    ) -> Result<ReadyData, async_graphql::Error> {
        let ready_state = ReadyData {
            playing: is_playing,
            position_secs,
        };

        let data = ctx.data::<Storage>()?;

        let rooms = &data.private_rooms;
        let mut room = rooms
            .get_mut(&room_id)
            .ok_or_else(|| async_graphql::Error::from("Room does not exist"))?;
        let user = room.get_player_mut(&user_id);
        match user {
            Some(user) => {
                user.state = UserState::Ready(ready_state.clone());
                {
                    let should_broadcast = room.users.iter().any(|user1| {
                        if let Some(userstate) = user1.state.as_ready() {
                            room.users.iter().any(|user2| {
                                if let Some(userstate2) = user2.state.as_ready() {
                                    userstate.playing != userstate2.playing
                                        || userstate
                                            .position_secs
                                            .abs_diff(userstate2.position_secs)
                                            > room.delay_difference_secs
                                } else {
                                    false
                                }
                            })
                        } else {
                            false
                        }
                    });
                    if should_broadcast {
                        room.broadcast(ServerResponse::StatusUpdate(ready_state.clone()))
                            .await;
                    }
                }
                Ok(ready_state)
            }
            None => Err(anyhow::anyhow!("User not found").into()),
        }
    }

    pub async fn paused<'ctx>(
        &self,
        ctx: &Context<'_>,
        user_id: String,
        room_id: String,
        position_secs: u64,
    ) -> Result<ReadyData, async_graphql::Error> {
        let ready_state = ReadyData {
            playing: false,
            position_secs,
        };

        let data = ctx.data::<Storage>()?;

        let rooms = &data.private_rooms;
        let mut room = rooms
            .get_mut(&room_id)
            .ok_or_else(|| async_graphql::Error::from("Room does not exist"))?;
        let user = room.get_player_mut(&user_id);
        match user {
            Some(user) => {
                user.state = UserState::Ready(ready_state.clone());
                {
                    room.broadcast(ServerResponse::StatusUpdate(ready_state.clone()))
                        .await;
                }
                Ok(ready_state)
            }
            None => Err(anyhow::anyhow!("User not found").into()),
        }
    }

    pub async fn resumed<'ctx>(
        &self,
        ctx: &Context<'_>,
        user_id: String,
        room_id: String,
        position_secs: u64,
    ) -> Result<ReadyData, async_graphql::Error> {
        let ready_state = ReadyData {
            playing: true,
            position_secs,
        };

        let data = ctx.data::<Storage>()?;

        let rooms = &data.private_rooms;
        let mut room = rooms
            .get_mut(&room_id)
            .ok_or_else(|| async_graphql::Error::from("Room does not exist"))?;
        let user = room.get_player_mut(&user_id);
        match user {
            Some(user) => {
                user.state = UserState::Ready(ready_state.clone());
                {
                    room.broadcast(ServerResponse::StatusUpdate(ready_state.clone()))
                        .await;
                }
                Ok(ready_state)
            }
            None => Err(anyhow::anyhow!("User not found").into()),
        }
    }
}

pub struct MutationRoot;

#[Object]
impl MutationRoot {
    pub async fn create_lobby<'ctx>(
        &self,
        ctx: &Context<'_>,
        user_id: String,
        user_name: String,
        delay_difference_secs: u64,
    ) -> Result<String, async_graphql::Error> {
        let data = ctx.data::<Storage>()?;
        let rooms = &data.private_rooms;
        let room_id = generate_rand_string(6);
        if rooms.contains_key(&room_id) {
            Err("Cant create room".into())
        } else {
            rooms.insert(
                room_id.clone(),
                Room::new(
                    room_id.clone(),
                    Player {
                        id: user_id,
                        name: user_name,
                    },
                    delay_difference_secs,
                ),
            );
            Ok(room_id)
        }
    }

    pub async fn join_lobby<'ctx>(
        &self,
        ctx: &Context<'_>,
        player_id: String,
        player_name: String,
        room_id: String,
    ) -> Result<String, async_graphql::Error> {
        let data = ctx.data::<Storage>()?;
        let player = Player {
            id: player_id,
            name: player_name,
        };
        let room = {
            let rooms = &data.private_rooms;

            let mut room = rooms
                .get_mut(&room_id)
                .ok_or_else(|| async_graphql::Error::from("Room does not exist"))?;

            room.add_player(player.clone())?;
            room.clone()
        };

        room.broadcast(ServerResponse::PlayerJoined(PlayerJoined {
            player: player.clone(),

            room: room.clone(),
        }))
        .await;
        room.broadcast(ServerResponse::ChatMessage(ChatMessage {
            message: format!("{} Joined", player.name),
            player: player,
            color: Some("#00FF00".into()),
        }))
        .await;
        Ok(room_id)
    }

    pub async fn disconnect<'ctx>(
        &self,
        ctx: &Context<'_>,
        player_id: String,
        room_id: String,
    ) -> Result<String, async_graphql::Error> {
        let data = ctx.data::<Storage>()?;

        let (room, player) = {
            let rooms = &data.private_rooms;

            let mut room = rooms
                .get_mut(&room_id)
                .ok_or_else(|| async_graphql::Error::from("Room does not exist"))?;

            let player = room.remove_player(&player_id)?;
            if room.is_empty() {
                rooms.remove(&room.id);
            }

            (room.clone(), player)
        };

        room.clone()
            .broadcast(ServerResponse::PlayerRemoved(PlayerRemoved {
                player: player.clone(),

                room: room.clone(),
            }))
            .await;
        room.broadcast(ServerResponse::ChatMessage(ChatMessage {
            message: format!("{} Removed", player.name),
            player: player.clone(),
            color: Some("#FF0000".into()),
        }))
        .await;
        Ok("Disconnected".into())
    }

    pub async fn chat<'ctx>(
        &self,
        ctx: &Context<'_>,
        player_id: String,
        room_id: String,
        message: String,
    ) -> Result<String, async_graphql::Error> {
        let data = ctx.data::<Storage>()?;

        let (room, player) = {
            let rooms = &data.private_rooms;

            let room = rooms
                .get_mut(&room_id)
                .ok_or_else(|| async_graphql::Error::from("Room does not exist"))?;

            let player = room
                .get_player(&player_id)
                .ok_or("Player not in room")?
                .clone();
            (room.clone(), player.player)
        };

        room.broadcast(ServerResponse::ChatMessage(ChatMessage {
            player,
            message,
            color: None,
        }))
        .await;
        Ok("Sucess".into())
    }
}

pub struct Subscription;

#[Subscription]
impl Subscription {
    async fn server_messages<'ctx>(
        &self,
        ctx: &Context<'_>,

        room_id: String,
        player_id: String,
    ) -> Result<impl Stream<Item = ServerResponse>, async_graphql::Error> {
        let (tx, rx) = channel::<ServerResponse>(2);

        let data = ctx.data::<Storage>()?;
        let room = {
            let rooms = &data.private_rooms;
            let mut room = rooms
                .get_mut(&room_id)
                .ok_or_else(|| async_graphql::Error::from("Room does not exist"))?;
            room.set_player_channel(player_id.clone(), tx)?;
            room.clone()
        };
        let player = room
            .get_player(&player_id)
            .ok_or("Player not found ")?
            .clone()
            .player;
        room.clone()
            .broadcast(ServerResponse::PlayerConnected(PlayerConnected {
                player: player.clone(),

                room: room.clone(),
            }))
            .await;
        room.broadcast(ServerResponse::ChatMessage(ChatMessage {
            message: format!("{} Connected", player.name),
            player: player.clone(),
            color: Some("#00FF00".into()),
        }))
        .await;
        let player_dis = PlayerDisconnected {
            player,
            receiver_stream: rx,
            rooms: ctx.data::<Storage>()?.private_rooms.clone(),
            room_id,
        };
        Ok(player_dis)
    }
}

pub struct PlayerDisconnected {
    player: Player,
    receiver_stream: Receiver<ServerResponse>,
    rooms: Arc<DashMap<String, Room>>,
    room_id: String,
}

impl Drop for PlayerDisconnected {
    fn drop(&mut self) {
        let rooms = self.rooms.clone();
        let room_id = self.room_id.clone();
        let player = self.player.clone();
        tokio::spawn(async move {
            {
                log::info!("Taking room to remove player {:#?}", player);
                let rooms = &rooms;
                log::info!("Removing player {:#?}", player);
                let mut remove = false;
                if let Some(mut room) = rooms.get_mut(&room_id) {
                    if let Err(er) = room.disconnect_player(&player.id) {
                        log::warn!("Could not remove player {:#?}", er)
                    } else {
                        log::info!("Player removed {:#?}", player);
                    }
                    if room.is_empty() {
                        remove = true;
                    } else {
                        log::info!("Sending broadcast PlayerLeft {:#?}", player);

                        log::info!("Updating Turn");

                        log::info!("Turn Updated")
                    }
                }
                if remove {
                    log::info!("Deleting room {:#?}", room_id);

                    rooms.remove(&room_id);
                    log::info!("Deleted room {:#?}", room_id);
                }
            }
            {
                let rooms = &rooms;
                if let Some(room) = rooms.get(&room_id) {
                    room.clone()
                        .broadcast(ServerResponse::PlayerLeft(PlayerLeft {
                            player: player.clone(),
                            room: room.clone(),
                        }))
                        .await;
                    room.broadcast(ServerResponse::ChatMessage(ChatMessage {
                        message: format!("{} Left", player.name),
                        player: player.clone(),
                        color: Some("#FF0000".into()),
                    }))
                    .await;
                }
            }
        });
    }
}

impl Stream for PlayerDisconnected {
    type Item = ServerResponse;

    fn poll_next(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        self.receiver_stream.poll_recv(cx)
    }
}
