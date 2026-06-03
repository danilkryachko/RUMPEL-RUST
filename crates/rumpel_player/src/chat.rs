use bevy::input::ButtonState;
use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::prelude::*;
use mlua::Table;
use rumpel_modding::LuaRuntime;
use rumpel_prelude::*;

#[derive(Resource, Default, Debug)]
pub struct ChatState {
    pub is_open: bool,
    pub input_text: String,
    pub messages: Vec<(String, String, Color)>, // (sender, message, color)
}

#[derive(Component)]
pub struct ChatContainer;

#[derive(Component)]
pub struct ChatMessageListText;

#[derive(Component)]
pub struct ChatInputRow;

#[derive(Component)]
pub struct ChatInputPrompt;

#[derive(Component)]
pub struct ChatInputLine;

type ChatInputTextQuery<'w, 's> = Query<
    'w,
    's,
    &'static mut Text,
    (
        With<ChatInputLine>,
        Without<ChatMessageListText>,
        Without<ChatInputRow>,
    ),
>;

pub struct RumpelChatPlugin;

impl Plugin for RumpelChatPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ChatState>()
            .add_systems(Startup, setup_chat_ui)
            .add_systems(
                Update,
                (
                    chat_input_system,
                    consume_chat_queue_system,
                    update_chat_ui_system,
                )
                    .run_if(in_state(GameState::InGame)),
            );
    }
}

fn setup_chat_ui(mut commands: Commands, camera_query: Query<Entity, With<crate::PlayerCamera>>) {
    let ui_camera = camera_query.iter().next();
    info!("CHAT: Spawning Chat HUD UI...");

    // 1. Create main chat container
    let mut container = commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            left: px(15),
            bottom: px(15),
            width: px(420),
            height: px(240),
            padding: UiRect::all(px(10)),
            flex_direction: FlexDirection::Column,
            justify_content: JustifyContent::FlexEnd,
            ..default()
        },
        BackgroundColor(Color::srgba(0.01, 0.015, 0.01, 0.68)),
        ChatContainer,
    ));

    if let Some(cam) = ui_camera {
        container.insert(UiTargetCamera(cam));
    }

    container.with_children(|parent| {
        // 2. Chat history text area
        parent.spawn((
            Text::new(""),
            TextFont::from_font_size(14.0),
            TextColor(Color::srgb(0.9, 0.95, 0.9)),
            Node {
                margin: UiRect::bottom(px(8)),
                max_width: px(400),
                ..default()
            },
            ChatMessageListText,
        ));

        // 3. Input row container (Prompt + Active text)
        parent
            .spawn((
                Node {
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::Center,
                    ..default()
                },
                ChatInputRow,
            ))
            .with_children(|row| {
                // Prompt prefix
                row.spawn((
                    Text::new("> "),
                    TextFont::from_font_size(15.0),
                    TextColor(Color::srgb(0.15, 0.72, 0.98)),
                    ChatInputPrompt,
                ));
                // Active typing text
                row.spawn((
                    Text::new(""),
                    TextFont::from_font_size(15.0),
                    TextColor(Color::srgb(0.95, 0.95, 0.95)),
                    ChatInputLine,
                ));
            });
    });

    info!(
        "CHAT: Successfully spawned Chat UI components attached to camera: {:?}",
        ui_camera
    );
}

fn chat_input_system(
    mut keyboard_input: ResMut<ButtonInput<KeyCode>>,
    mut key_events: MessageReader<KeyboardInput>,
    mut chat_state: ResMut<ChatState>,
    lua_runtime: Option<Res<LuaRuntime>>,
) {
    // 1. Toggle Chat Open/Close on Enter key press
    if keyboard_input.just_pressed(KeyCode::Enter) {
        if !chat_state.is_open {
            // Open chat
            chat_state.is_open = true;
            chat_state.input_text.clear();
            // Clear standard keyboard input buffer so movement triggers don't immediately fire on release
            keyboard_input.reset_all();
            info!("CHAT: Opened chat keyboard focus");
        } else {
            // Send message / execute command
            let raw_text = chat_state.input_text.trim().to_string();
            if !raw_text.is_empty()
                && let Some(ref lua_res) = lua_runtime
                && let Ok(lua) = lua_res.0.lock()
            {
                let globals = lua.globals();
                if let Some(cmd_part) = raw_text.strip_prefix('/') {
                    // Extract command and arguments
                    let parts: Vec<&str> = cmd_part.splitn(2, ' ').collect();
                    let cmd = parts[0].to_string();
                    let args = parts.get(1).copied().unwrap_or("").to_string();

                    if let Ok(func) = globals.get::<mlua::Function>("trigger_chat_command")
                        && let Err(e) = func.call::<()>((cmd.clone(), args))
                    {
                        error!("CHAT: Error running command /{}: {:?}", cmd, e);
                    }
                } else {
                    // Regular chat message
                    if let Ok(func) = globals.get::<mlua::Function>("trigger_chat_message")
                        && let Err(e) = func.call::<()>(("Player".to_string(), raw_text))
                    {
                        error!("CHAT: Error triggering chat message: {:?}", e);
                    }
                }
            }
            chat_state.is_open = false;
            chat_state.input_text.clear();
            keyboard_input.reset_all();
            info!("CHAT: Closed chat keyboard focus");
        }
        return;
    }

    // 2. Capture typing input if chat is open
    if chat_state.is_open {
        for event in key_events.read() {
            // Only process pressed state, ignore repeats and releases
            if event.state == ButtonState::Pressed && !event.repeat {
                match &event.logical_key {
                    Key::Backspace => {
                        chat_state.input_text.pop();
                    }
                    Key::Escape => {
                        chat_state.is_open = false;
                        chat_state.input_text.clear();
                        keyboard_input.reset_all();
                        info!("CHAT: Aborted typing input via Escape");
                        return;
                    }
                    Key::Enter => {
                        // Enter handles send/close separately to toggle cleanly
                    }
                    Key::Character(ch_str) if chat_state.input_text.chars().count() < 42 => {
                        chat_state.input_text.push_str(ch_str.as_str());
                    }
                    _ => {}
                }
            }
        }
    }
}

fn consume_chat_queue_system(
    mut chat_state: ResMut<ChatState>,
    lua_runtime: Option<Res<LuaRuntime>>,
) {
    let Some(ref lua_res) = lua_runtime else {
        return;
    };
    let Ok(lua) = lua_res.0.lock() else {
        return;
    };

    let globals = lua.globals();
    let Ok(queue) = globals.get::<Table>("ChatMessageQueue") else {
        return;
    };

    let len = queue.len().unwrap_or(0);
    if len == 0 {
        return;
    }

    let mut new_messages = Vec::new();
    for i in 1..=len {
        if let Ok(entry) = queue.get::<Table>(i)
            && let (Ok(sender), Ok(text), Ok(color_tbl)) = (
                entry.get::<String>("sender"),
                entry.get::<String>("text"),
                entry.get::<Table>("color"),
            )
        {
            let r: f32 = color_tbl.get(1).unwrap_or(1.0);
            let g: f32 = color_tbl.get(2).unwrap_or(1.0);
            let b: f32 = color_tbl.get(3).unwrap_or(1.0);
            let color = Color::srgb(r, g, b);
            new_messages.push((sender, text, color));
        }
    }

    // Clear queue in Lua
    let _ = lua.load("ChatMessageQueue = {}").exec();

    // Push new messages into our Bevy ChatState resource
    for (sender, text, color) in new_messages {
        if sender == "System" && text == "CLEAR_CHAT_LOG" {
            chat_state.messages.clear();
            continue;
        }
        chat_state.messages.push((sender, text, color));
        // Cap messages history log to prevent memory leak
        if chat_state.messages.len() > 100 {
            chat_state.messages.remove(0);
        }
    }
}

fn update_chat_ui_system(
    chat_state: Res<ChatState>,
    mut container_query: Query<&mut BackgroundColor, With<ChatContainer>>,
    mut history_query: Query<&mut Text, (With<ChatMessageListText>, Without<ChatInputLine>)>,
    mut row_query: Query<&mut Visibility, (With<ChatInputRow>, Without<ChatMessageListText>)>,
    mut input_query: ChatInputTextQuery,
) {
    // 1. Container visual presence (dim background when closed, dark when active)
    for mut bg in &mut container_query {
        if chat_state.is_open {
            bg.0 = Color::srgba(0.01, 0.015, 0.01, 0.82);
        } else {
            // Keep container entirely transparent when closed unless there are active messages in history
            if chat_state.messages.is_empty() {
                bg.0 = Color::srgba(0.0, 0.0, 0.0, 0.0);
            } else {
                bg.0 = Color::srgba(0.01, 0.015, 0.01, 0.35);
            }
        }
    }

    // 2. Message History text compilation (last 8 messages)
    let history_len = chat_state.messages.len();
    let start_idx = history_len.saturating_sub(8);
    let mut compiled_text = String::new();

    for i in start_idx..history_len {
        let (sender, text, _) = &chat_state.messages[i];
        if sender.is_empty() {
            compiled_text.push_str(&format!("{text}\n"));
        } else {
            compiled_text.push_str(&format!("{sender}: {text}\n"));
        }
    }

    for mut text_node in &mut history_query {
        if text_node.0 != compiled_text {
            text_node.0 = compiled_text.clone();
        }
    }

    // 3. Input Row Visibility
    for mut vis in &mut row_query {
        let target_vis = if chat_state.is_open {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
        if *vis != target_vis {
            *vis = target_vis;
        }
    }

    // 4. Update typing input string
    for mut text_node in &mut input_query {
        if text_node.0 != chat_state.input_text {
            text_node.0 = chat_state.input_text.clone();
        }
    }
}
