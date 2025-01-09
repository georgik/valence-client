use bytes::Buf;
use std::io::{self};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use uuid::Uuid;
use valence::protocol::packets::handshaking::HandshakeC2s;
use valence::protocol::packets::login::{LoginHelloC2s, LoginSuccessS2c, LoginCompressionS2c};
use valence::protocol::packets::play::{GameJoinS2c, KeepAliveS2c, PlayerPositionLookS2c, PlayerAbilitiesS2c, ChunkDataS2c, ChatMessageS2c, DisconnectS2c, EntityStatusS2c, PlayerListS2c, PlayerRespawnS2c, PlayerSpawnPositionS2c, CommandTreeS2c, UpdateSelectedSlotS2c, AdvancementUpdateS2c, HealthUpdateS2c, EntityAttributesS2c, SynchronizeTagsS2c};
use valence::protocol::packets::status::{QueryRequestC2s, QueryResponseS2c};
use valence::protocol::packets::handshaking::handshake_c2s::HandshakeNextState;
use valence::protocol::{Decode, Encode, Packet, PacketDecoder, PacketEncoder};
use valence::protocol::VarInt;

#[tokio::main]
async fn main() -> io::Result<()> {
    let server_address = "127.0.0.1:25565";

    // Connect to the server
    let tcp_stream = TcpStream::connect(server_address).await?;
    tcp_stream.set_nodelay(true)?;
    println!("Connected to server at {}", server_address);

    let mut conn = tcp_stream;
    let mut dec = PacketDecoder::new();
    let mut enc = PacketEncoder::new();

    // Step 1: Send Handshake
    send_handshake(&mut conn, &mut enc, HandshakeNextState::Login).await?;

    // Step 2: Log in and handle updates
    login_and_handle_updates(&mut conn, &mut dec, &mut enc).await?;

    Ok(())
}

/// Sends a handshake packet with the given next state.
async fn send_handshake(conn: &mut TcpStream, enc: &mut PacketEncoder, next_state: HandshakeNextState) -> io::Result<()> {
    let handshake_packet = HandshakeC2s {
        protocol_version: VarInt(763), // Protocol version for Minecraft 1.20
        server_address: "127.0.0.1".into(),
        server_port: 25565,
        next_state,
    };

    enc.append_packet(&handshake_packet).expect("Failed to encode handshake packet");
    conn.write_all(&enc.take()).await?;
    Ok(())
}

/// Logs into the server and handles updates after login.
async fn login_and_handle_updates(conn: &mut TcpStream, dec: &mut PacketDecoder, enc: &mut PacketEncoder) -> io::Result<()> {
    let login_start_packet = LoginHelloC2s {
        username: "Player".into(), // Only username is required in offline mode
        profile_id: None,          // No profile ID in offline mode
    };

    enc.append_packet(&login_start_packet).expect("Failed to encode LoginHelloC2s packet");
    conn.write_all(&enc.take()).await?;
    println!("Login request sent.");

    let mut buf = vec![0u8; 4096];
    loop {
        let bytes_read = conn.read(&mut buf).await?;
        if bytes_read == 0 {
            println!("Connection closed by server.");
            return Ok(());
        }

        dec.queue_bytes(buf[..bytes_read].into());

        while let Ok(Some(frame)) = dec.try_next_packet() {
            match frame.id {
                LoginCompressionS2c::ID => {
                    let packet: LoginCompressionS2c = frame.decode().expect("Failed to decode LoginCompressionS2c");
                    println!("Received LoginCompressionS2c packet with threshold: {}", packet.threshold.0);
                    dec.set_compression(valence::protocol::CompressionThreshold(packet.threshold.0));
                }
                LoginSuccessS2c::ID => {
                    let packet: LoginSuccessS2c = frame.decode().expect("Failed to decode LoginSuccessS2c");
                    println!("Login successful! Username: {}, UUID: {}", packet.username, packet.uuid);
                }
                GameJoinS2c::ID => {
                    let packet: GameJoinS2c = frame.decode().expect("Failed to decode GameJoinS2c");
                    println!("Joined the game world with Entity ID: {}", packet.entity_id);
                }
                PlayerPositionLookS2c::ID => {
                    let packet: PlayerPositionLookS2c = frame.decode().expect("Failed to decode PlayerPositionLookS2c");
                    println!(
                        "Player position updated: x={}, y={}, z={}, yaw={}, pitch={}",
                        packet.position.x, packet.position.y, packet.position.z, packet.yaw, packet.pitch
                    );
                }
                PlayerAbilitiesS2c::ID => {
                    println!("Player abilities updated.");
                }
                ChunkDataS2c::ID => {
                    println!("Received chunk data.");
                }
                ChatMessageS2c::ID => {
                    let packet: ChatMessageS2c = frame.decode().expect("Failed to decode ChatMessageS2c");
                    println!("Chat message: {}", packet.message);
                }
                KeepAliveS2c::ID => {
                    let packet: KeepAliveS2c = frame.decode().expect("Failed to decode KeepAliveS2c");
                    println!("KeepAlive received with ID: {}", packet.id);
                }
                DisconnectS2c::ID => {
                    let packet: DisconnectS2c = frame.decode().expect("Failed to decode DisconnectS2c");
                    println!("Disconnected by server: {}", packet.reason);
                    return Ok(());
                }
                EntityStatusS2c::ID => {
                    println!("Entity status updated.");
                }
                PlayerListS2c::ID => {
                    println!("Player list updated.");
                }
                PlayerRespawnS2c::ID => {
                    println!("Player respawned.");
                }
                PlayerSpawnPositionS2c::ID => {
                    println!("Player spawn position updated.");
                }
                CommandTreeS2c::ID => {
                    let packet: CommandTreeS2c = frame.decode().expect("Failed to decode CommandTreeS2c");
                    println!("Received CommandTreeS2c packet. Root node ID");
                }
                UpdateSelectedSlotS2c::ID => {
                    let packet: UpdateSelectedSlotS2c = frame.decode().expect("Failed to decode UpdateSelectedSlotS2c");
                    println!("Received UpdateSelectedSlotS2c packet. Selected slot: {}", packet.slot);
                }
                AdvancementUpdateS2c::ID => {
                    let packet: AdvancementUpdateS2c = frame.decode().expect("Failed to decode AdvancementUpdateS2c");
                    println!("Received advancement update. Number of advancements: {}", packet.advancement_mapping.len());
                }
                HealthUpdateS2c::ID => {
                    let packet: HealthUpdateS2c = frame.decode().expect("Failed to decode HealthUpdateS2CPacket");
                    println!(
                        "Health Update: health={},  saturation={}",
                        packet.health,  packet.food_saturation
                    );
                }
                EntityAttributesS2c::ID => {
                    let packet: EntityAttributesS2c = frame.decode().expect("Failed to decode EntityAttributesS2CPacket");
                    println!(
                        "Entity Attributes Update: Entity ID = {:?}, Attributes = {:?}",
                        packet.entity_id, packet.properties
                    );
                }
                SynchronizeTagsS2c::ID => {
                    let packet: SynchronizeTagsS2c = frame.decode().expect("Failed to decode SynchronizeTagsS2c packet");
                    println!("Synchronize tags");
                    // println!("Synchronize Tags Packet: {:?}", packet);
                }


                _ => println!("Unhandled packet ID during login/update: {}", frame.id),
            }
        }
    }
}

