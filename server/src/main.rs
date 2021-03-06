#[macro_use]
extern crate log;

use std::{
    collections::HashMap,
    rc::Rc,
    time::Duration,
};

use simple_logger::SimpleLogger;
use smol::io;

use naia_server::{ActorKey, NaiaServer, ServerAddresses, Random, ServerConfig, ServerEvent, UserKey};
use log::LevelFilter;
use ascii_game_shared::{
    get_shared_config, manifest_load, actors::{PointActor, PointActorColor, WorldActor}, Events, Actors, shared_behaviour,
};
use ascii_game_shared::game::map::Map;

fn main() -> io::Result<()> {
    let server_addresses: ServerAddresses = ServerAddresses::new(
        // IP Address to listen on for the signaling portion of WebRTC
        "0.0.0.0:14191"
            .parse()
            .expect("could not parse HTTP address/port"),
        // IP Address to listen on for UDP WebRTC data channels
        "0.0.0.0:14192"
            .parse()
            .expect("could not parse WebRTC data address/port"),
        // The public WebRTC IP address to advertise
        "0.0.0.0:14192"
            .parse()
            .expect("could not parse advertised public WebRTC data address/port"),
    );
    smol::block_on(async {
        SimpleLogger::new().with_level(LevelFilter::Info).init().expect("A logger was already initialized");


        let mut server_config = ServerConfig::default();
        server_config.heartbeat_interval = Duration::from_secs(2);
        server_config.disconnection_timeout_duration = Duration::from_secs(10000);

        let mut server = NaiaServer::new(
            server_addresses,
            manifest_load(),
            Some(server_config),
            get_shared_config(),
        ).await;

        info!("Server started.");


        server.on_auth(Rc::new(Box::new(|_, auth_type| {
            if let Events::AuthEvent(auth_event) = auth_type {
                let username = auth_event.username.get();
                let password = auth_event.password.get();
                return true;
                return username == "charlie" && password == "12345";
            }
            return false;
        })));

        let main_room_key = server.create_room();

        server.on_scope_actor(Rc::new(Box::new(|_, _, _, actor| match actor {
            Actors::PointActor(_) => {
                return true;
            },
            Actors::WorldActor(_) => {
                return true;
            },
            Actors::ChatActor(_) => {
                return true;
            }
        })));

        let mut user_to_pawn_map = HashMap::<UserKey, ActorKey>::new();

        let mut map = Map::default();
        let world = WorldActor::new(map.seed).wrap();

        let world_key = server
            .register_actor(Actors::WorldActor(world.clone()));
        server.room_add_actor(&main_room_key, &world_key);
        info!("Created world actor.");
        info!("Seed: {}", map.seed);

        loop {
            match server.receive().await {
                Ok(event) => {
                    match event {
                        ServerEvent::Connection(user_key) => {
                            info!("Incoming connection...");
                            server.room_add_user(&main_room_key, &user_key);
                            if let Some(user) = server.get_user(&user_key) {
                                info!("Server connected to: {}", user.address);

                                let x = Random::gen_range_u32(0, 50);
                                let y = Random::gen_range_u32(0, 37);

                                let actor_color = match server.get_users_count() % 3 {
                                    0 => PointActorColor::Yellow,
                                    1 => PointActorColor::Red,
                                    _ => PointActorColor::Blue,
                                };

                                let new_actor =
                                    PointActor::new(x as i32, y as i32, actor_color).wrap();
                                let new_actor_key = server
                                    .register_actor(Actors::PointActor(new_actor.clone()));
                                server.room_add_actor(&main_room_key, &new_actor_key);
                                server.assign_pawn(&user_key, &new_actor_key);
                                user_to_pawn_map.insert(user_key, new_actor_key);
                            }
                        }
                        ServerEvent::Disconnection(user_key, user) => {
                            info!("Naia Server disconnected from: {:?}", user.address);
                            server.room_remove_user(&main_room_key, &user_key);
                            if let Some(actor_key) = user_to_pawn_map.remove(&user_key) {
                                server.room_remove_actor(&main_room_key, &actor_key);
                                server.unassign_pawn(&user_key, &actor_key);
                                server.deregister_actor(actor_key);
                            }
                        }
                        ServerEvent::Command(_, actor_key, command_type) => if let Events::KeyCommand(key_command) = command_type {
                            if let Some(typed_actor) = server.get_actor(actor_key) {
                                if let Actors::PointActor(actor) = typed_actor {
                                    shared_behaviour::process_command(&key_command, actor);
                                }
                            }
                        },
                        ServerEvent::Event(user_key, event) => if let Events::ChatEvent(chat) = event {
                            if let Some(user) = server.get_user(&user_key) {
                                info!("chat received: {}", chat.body.get());
                                for x in user_to_pawn_map.keys() {
                                    server.queue_event(x, &chat);
                                }
                            }
                        }
                        ServerEvent::Tick => {
                            server.send_all_updates().await;
                            //info!("tick");
                        }
                        _ => {}
                    }
                }
                Err(error) => {
                    info!("Naia Server Error: {}", error);
                }
            }
        }
    })
}