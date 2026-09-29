//! Native 3D Battle Chess.
//!
//! This binary keeps the web game's core idea—local chess with optional CPU,
//! clocks, appearance controls, captured-piece display, and battle capture
//! effects—but runs as a standalone Bevy desktop application.

mod cpu;
mod game;

use std::f32::consts::PI;

use bevy::{
    asset::RenderAssetUsages,
    input::mouse::{AccumulatedMouseScroll, MouseScrollUnit},
    math::primitives::InfinitePlane3d,
    mesh::PrimitiveTopology,
    prelude::*,
    window::{PrimaryWindow, WindowResolution},
};
use bevy_egui::{EguiContexts, EguiPlugin, EguiPrimaryContextPass, egui};
use shakmaty::{Color as ChessColor, Move, Piece as ChessPiece, Position, Role, Square};

use cpu::choose_cpu_move;
use game::{
    BattleGame, capture_square, color_name, display_move_to, move_from, square_from_indices,
    square_indices, status_text,
};

const CPU_DELAY_SECONDS: f32 = 0.42;
const BOARD_Y: f32 = 0.0;
const ATTACK_ROLES: [Role; 6] = [
    Role::Pawn,
    Role::Knight,
    Role::Bishop,
    Role::Rook,
    Role::Queen,
    Role::King,
];

fn main() {
    App::new()
        .insert_resource(ClearColor(Color::srgb(0.56, 0.67, 0.74)))
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Battle Chess 3D — Rust".to_owned(),
                resolution: WindowResolution::new(1320, 860),
                resize_constraints: bevy::window::WindowResizeConstraints {
                    min_width: 900.0,
                    min_height: 620.0,
                    ..default()
                },
                ..default()
            }),
            ..default()
        }))
        .add_plugins(EguiPlugin::default())
        .init_resource::<BattleGame>()
        .init_resource::<InteractionState>()
        .init_resource::<CpuSettings>()
        .init_resource::<ChessClock>()
        .init_resource::<Appearance>()
        .init_resource::<BattleSettings>()
        .init_resource::<CameraRig>()
        .init_resource::<CameraGesture>()
        .init_resource::<SceneSync>()
        .init_resource::<UiCapture>()
        .init_resource::<FxPause>()
        .add_systems(Startup, setup_scene)
        .add_systems(
            EguiPrimaryContextPass,
            (hud_system, camera_pointer_system, board_input_system).chain(),
        )
        .add_systems(
            Update,
            (
                cpu_turn_system,
                clock_system,
                appearance_system,
                capture_fx_system,
                sync_scene_system,
                camera_system,
            )
                .chain(),
        )
        .run();
}

// ============================================================
// Resources
// ============================================================

#[derive(Resource)]
struct InteractionState {
    selected: Option<Square>,
    legal_destinations: Vec<Square>,
    keyboard_cursor: Square,
    pending_promotion: Option<Vec<Move>>,
    show_legal_moves: bool,
}

impl Default for InteractionState {
    fn default() -> Self {
        Self {
            selected: None,
            legal_destinations: Vec::new(),
            keyboard_cursor: Square::E2,
            pending_promotion: None,
            show_legal_moves: true,
        }
    }
}

impl InteractionState {
    fn clear_selection(&mut self) {
        self.selected = None;
        self.legal_destinations.clear();
    }

    fn reset(&mut self) {
        *self = Self::default();
    }
}

#[derive(Resource)]
struct CpuSettings {
    enabled: bool,
    color: ChessColor,
    delay_remaining: f32,
}

impl Default for CpuSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            color: ChessColor::Black,
            delay_remaining: CPU_DELAY_SECONDS,
        }
    }
}

#[derive(Resource)]
struct ChessClock {
    enabled: bool,
    initial_seconds: f32,
    white_seconds: f32,
    black_seconds: f32,
    timed_out: Option<ChessColor>,
}

impl Default for ChessClock {
    fn default() -> Self {
        Self {
            enabled: false,
            initial_seconds: 5.0 * 60.0,
            white_seconds: 5.0 * 60.0,
            black_seconds: 5.0 * 60.0,
            timed_out: None,
        }
    }
}

impl ChessClock {
    fn reset(&mut self) {
        self.white_seconds = self.initial_seconds;
        self.black_seconds = self.initial_seconds;
        self.timed_out = None;
    }

    fn set_minutes(&mut self, minutes: f32) {
        self.initial_seconds = minutes.clamp(0.25, 180.0) * 60.0;
        self.reset();
    }

    fn remaining_mut(&mut self, color: ChessColor) -> &mut f32 {
        match color {
            ChessColor::White => &mut self.white_seconds,
            ChessColor::Black => &mut self.black_seconds,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum BoardPreset {
    Walnut,
    Marble,
    Tournament,
    Slate,
    Obsidian,
    Neon,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PiecePreset {
    Ivory,
    Walnut,
    Brass,
    Chrome,
    Glass,
    Neon,
}

#[derive(Resource)]
struct Appearance {
    board_preset: BoardPreset,
    piece_preset: PiecePreset,
    light_square: [f32; 3],
    dark_square: [f32; 3],
    background: [f32; 3],
    light_color: [f32; 3],
    white_piece: [f32; 3],
    black_piece: [f32; 3],
    board_roughness: f32,
    board_metallic: f32,
    piece_roughness: f32,
    piece_metallic: f32,
    glossy: bool,
    show_captured: bool,
    captured_upright: bool,
    knight_orientation: i32,
}

impl Default for Appearance {
    fn default() -> Self {
        Self {
            board_preset: BoardPreset::Walnut,
            piece_preset: PiecePreset::Ivory,
            light_square: hex_rgb(0xd7bd96),
            dark_square: hex_rgb(0x6d4327),
            background: hex_rgb(0x8faabd),
            light_color: hex_rgb(0xfff8ee),
            white_piece: hex_rgb(0xefe8d7),
            black_piece: hex_rgb(0x252525),
            board_roughness: 0.58,
            board_metallic: 0.01,
            piece_roughness: 0.62,
            piece_metallic: 0.02,
            glossy: false,
            show_captured: true,
            captured_upright: false,
            knight_orientation: 0,
        }
    }
}

impl Appearance {
    fn apply_board_preset(&mut self) {
        let (light, dark, roughness, metallic) = match self.board_preset {
            BoardPreset::Walnut => (0xd7bd96, 0x6d4327, 0.58, 0.01),
            BoardPreset::Marble => (0xe4e2dd, 0x5d6571, 0.32, 0.03),
            BoardPreset::Tournament => (0xe8e2cf, 0x4f765c, 0.72, 0.0),
            BoardPreset::Slate => (0xb7bec5, 0x3a4249, 0.88, 0.02),
            BoardPreset::Obsidian => (0xc8cbd0, 0x17191d, 0.20, 0.16),
            BoardPreset::Neon => (0x7ce9e3, 0x46316e, 0.28, 0.18),
        };
        self.light_square = hex_rgb(light);
        self.dark_square = hex_rgb(dark);
        self.board_roughness = roughness;
        self.board_metallic = metallic;
    }

    fn apply_piece_preset(&mut self) {
        let (white, black, roughness, metallic) = match self.piece_preset {
            PiecePreset::Ivory => (0xefe8d7, 0x252525, 0.62, 0.02),
            PiecePreset::Walnut => (0xc7935f, 0x4a2b19, 0.74, 0.0),
            PiecePreset::Brass => (0xe1bd62, 0x414750, 0.28, 0.82),
            PiecePreset::Chrome => (0xe4ebf2, 0x5b6570, 0.18, 0.95),
            PiecePreset::Glass => (0xd6f4ff, 0x466173, 0.10, 0.12),
            PiecePreset::Neon => (0x72fff0, 0xff4fd8, 0.22, 0.30),
        };
        self.white_piece = hex_rgb(white);
        self.black_piece = hex_rgb(black);
        self.piece_roughness = roughness;
        self.piece_metallic = metallic;
    }
}

#[derive(Resource)]
struct BattleSettings {
    animations: bool,
}

impl Default for BattleSettings {
    fn default() -> Self {
        Self { animations: true }
    }
}

#[derive(Resource)]
struct CameraRig {
    orbit: f32,
    distance: f32,
    elevation: f32,
    flipped: bool,
}

impl Default for CameraRig {
    fn default() -> Self {
        Self {
            orbit: 0.0,
            distance: 14.4,
            elevation: 35.0_f32.to_radians(),
            flipped: false,
        }
    }
}

impl CameraRig {
    fn drag(&mut self, delta: Vec2) {
        self.orbit -= delta.x * 0.008;
        self.elevation =
            (self.elevation + delta.y * 0.006).clamp(15.0_f32.to_radians(), 80.0_f32.to_radians());
    }

    fn zoom(&mut self, amount: f32) {
        self.distance = (self.distance * (-amount * 0.12).exp()).clamp(7.0, 24.0);
    }
}

#[derive(Resource, Default)]
struct CameraGesture {
    start: Option<Vec2>,
    last: Option<Vec2>,
    dragging: bool,
    click: Option<Vec2>,
}

impl CameraGesture {
    fn update(
        &mut self,
        cursor: Option<Vec2>,
        pressed: bool,
        just_pressed: bool,
        just_released: bool,
        over_game: bool,
    ) -> Vec2 {
        self.click = None;
        let Some(cursor) = cursor else {
            *self = Self::default();
            return Vec2::ZERO;
        };
        if just_pressed && over_game {
            self.start = Some(cursor);
            self.last = Some(cursor);
            self.dragging = false;
        }
        let mut delta = Vec2::ZERO;
        if let Some(start) = self.start {
            self.dragging |= start.distance(cursor) > 6.0 || !over_game;
            if self.dragging && over_game && (pressed || just_released) {
                delta = cursor - self.last.unwrap_or(cursor);
            }
            self.last = Some(cursor);
            if just_released && !self.dragging && over_game {
                self.click = Some(cursor);
            }
        }
        if !pressed {
            self.start = None;
            self.last = None;
            self.dragging = false;
        }
        delta
    }
}

#[derive(Resource, Default)]
struct SceneSync {
    dirty: bool,
    undo_used: bool,
    pending_fx: Option<CaptureFxRequest>,
    pending_move: Option<Move>,
    exploding: bool,
    clear_fx: bool,
}

impl SceneSync {
    fn can_undo(&self, game: &BattleGame) -> bool {
        !self.undo_used && (self.pending_move.is_some() || game.can_undo())
    }

    fn cancel_capture(&mut self, fx_pause: &mut FxPause) {
        self.pending_fx = None;
        self.pending_move = None;
        self.exploding = false;
        self.clear_fx = true;
        self.dirty = true;
        fx_pause.remaining = 0.0;
    }
}

#[derive(Resource, Default)]
struct UiCapture {
    pointer: bool,
    keyboard: bool,
    game_right: f32,
}

#[derive(Resource, Default)]
struct FxPause {
    remaining: f32,
}

#[derive(Resource)]
struct SceneHandles {
    cube: Handle<Mesh>,
    sphere: Handle<Mesh>,
    bishop_mitre: Handle<Mesh>,
    queen_circlet: Handle<Mesh>,
    light_square: Handle<StandardMaterial>,
    dark_square: Handle<StandardMaterial>,
    board_base: Handle<StandardMaterial>,
    white_piece: Handle<StandardMaterial>,
    black_piece: Handle<StandardMaterial>,
    selected: Handle<StandardMaterial>,
    legal: Handle<StandardMaterial>,
    capture: Handle<StandardMaterial>,
    cursor: Handle<StandardMaterial>,
    fx: [Handle<StandardMaterial>; 6],
}

impl SceneHandles {
    fn attack_material(&self, role: Role) -> Handle<StandardMaterial> {
        self.fx[ATTACK_ROLES
            .iter()
            .position(|candidate| *candidate == role)
            .unwrap()]
        .clone()
    }
}

#[derive(Clone, Copy)]
struct CaptureFxRequest {
    from: Square,
    target: Square,
    role: Role,
}

// ============================================================
// Scene components
// ============================================================

#[derive(Component)]
struct MainCamera;

#[derive(Component)]
struct KeyLight;

#[derive(Component)]
struct PieceVisual;

#[derive(Component)]
struct HighlightVisual;

#[derive(Component)]
struct Projectile {
    start: Vec3,
    end: Vec3,
    elapsed: f32,
    duration: f32,
    arc: f32,
    role: Role,
    index: usize,
}

#[derive(Component)]
struct Debris {
    velocity: Vec3,
    remaining: f32,
}

// ============================================================
// Startup
// ============================================================

fn setup_scene(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    appearance: Res<Appearance>,
    mut scene_sync: ResMut<SceneSync>,
) {
    let cube = meshes.add(Cuboid::default());

    let light_square = materials.add(board_material(
        rgb(appearance.light_square),
        appearance.board_roughness,
        appearance.board_metallic,
    ));
    let dark_square = materials.add(board_material(
        rgb(appearance.dark_square),
        appearance.board_roughness,
        appearance.board_metallic,
    ));
    let board_base = materials.add(StandardMaterial {
        base_color: Color::srgb(0.08, 0.06, 0.05),
        perceptual_roughness: 0.48,
        metallic: 0.08,
        ..default()
    });
    let white_piece = materials.add(piece_material(
        rgb(appearance.white_piece),
        appearance.piece_roughness,
        appearance.piece_metallic,
        appearance.glossy,
    ));
    let black_piece = materials.add(piece_material(
        rgb(appearance.black_piece),
        appearance.piece_roughness,
        appearance.piece_metallic,
        appearance.glossy,
    ));

    let selected = materials.add(StandardMaterial {
        base_color: Color::srgb(1.0, 0.63, 0.12),
        perceptual_roughness: 0.35,
        ..default()
    });
    let legal = materials.add(StandardMaterial {
        base_color: Color::srgb(0.18, 0.78, 0.36),
        perceptual_roughness: 0.45,
        ..default()
    });
    let capture = materials.add(StandardMaterial {
        base_color: Color::srgb(0.94, 0.18, 0.12),
        perceptual_roughness: 0.38,
        ..default()
    });
    let cursor = materials.add(StandardMaterial {
        base_color: Color::srgb(0.12, 0.80, 0.92),
        perceptual_roughness: 0.35,
        ..default()
    });
    let fx = create_attack_materials(&mut materials);

    let handles = SceneHandles {
        cube: cube.clone(),
        sphere: meshes.add(Sphere::new(0.5)),
        bishop_mitre: meshes.add(bishop_mitre_mesh()),
        queen_circlet: meshes.add(Torus::new(0.20, 0.29)),
        light_square: light_square.clone(),
        dark_square: dark_square.clone(),
        board_base,
        white_piece,
        black_piece,
        selected,
        legal,
        capture,
        cursor,
        fx,
    };

    commands.spawn((
        Mesh3d(cube.clone()),
        MeshMaterial3d(handles.board_base.clone()),
        Transform::from_xyz(0.0, -0.12, 0.0).with_scale(Vec3::new(9.2, 0.20, 9.2)),
    ));

    for rank in 0..8 {
        for file in 0..8 {
            let is_light = (file + rank) % 2 == 0;
            commands.spawn((
                Mesh3d(cube.clone()),
                MeshMaterial3d(if is_light {
                    light_square.clone()
                } else {
                    dark_square.clone()
                }),
                Transform::from_xyz(file as f32 - 3.5, BOARD_Y, 3.5 - rank as f32)
                    .with_scale(Vec3::new(0.98, 0.12, 0.98)),
            ));
        }
    }

    commands.spawn((
        DirectionalLight {
            color: rgb(appearance.light_color),
            illuminance: 16_000.0,
            shadow_maps_enabled: true,
            ..default()
        },
        Transform::from_xyz(5.0, 10.0, 6.0).looking_at(Vec3::ZERO, Vec3::Y),
        KeyLight,
    ));

    commands.spawn((
        PointLight {
            intensity: 1_100_000.0,
            range: 28.0,
            ..default()
        },
        Transform::from_xyz(-6.0, 7.0, -5.0),
    ));

    commands.spawn((
        Camera3d::default(),
        Camera {
            clear_color: rgb(appearance.background).into(),
            ..default()
        },
        Transform::from_xyz(0.0, 8.7, 11.8).looking_at(Vec3::new(0.0, 0.4, 0.0), Vec3::Y),
        MainCamera,
    ));

    commands.insert_resource(handles);
    scene_sync.dirty = true;
}

// ============================================================
// User interface
// ============================================================

#[allow(clippy::too_many_arguments)]
fn hud_system(
    mut contexts: EguiContexts,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut game: ResMut<BattleGame>,
    mut interaction: ResMut<InteractionState>,
    mut cpu: ResMut<CpuSettings>,
    mut clock: ResMut<ChessClock>,
    mut appearance: ResMut<Appearance>,
    mut battle: ResMut<BattleSettings>,
    mut camera: ResMut<CameraRig>,
    mut scene_sync: ResMut<SceneSync>,
    mut ui_capture: ResMut<UiCapture>,
    mut fx_pause: ResMut<FxPause>,
) -> Result {
    let ctx = contexts.ctx_mut()?;
    let (panel_width, button_height, font_size) = sidebar_dimensions(ctx.viewport_rect().size());
    let point_scale = windows
        .iter()
        .next()
        .map_or(1.0, |window| ctx.pixels_per_point() / window.scale_factor());
    ui_capture.game_right = (ctx.viewport_rect().right() - panel_width) * point_scale;

    let mut viewport_ui = egui::Ui::new(
        ctx.clone(),
        "battle-chess-root".into(),
        egui::UiBuilder::new()
            .layer_id(egui::LayerId::background())
            .max_rect(ctx.viewport_rect()),
    );

    egui::Panel::right("battle-chess-controls")
        .exact_size(panel_width)
        .resizable(false)
        .show(&mut viewport_ui, |ui| {
            style_sidebar(ui, button_height, font_size);
            egui::ScrollArea::vertical()
                .id_salt("sidebar-scroll")
                .show(ui, |ui| {
                    ui.heading("Battle Chess 3D");
                    ui.label(status_text(&game));

                    if let Some(loser) = clock.timed_out {
                        ui.colored_label(
                            egui::Color32::LIGHT_RED,
                            format!(
                                "{} wins on time",
                                color_name(match loser {
                                    ChessColor::White => ChessColor::Black,
                                    ChessColor::Black => ChessColor::White,
                                })
                            ),
                        );
                    }

                    ui.separator();
                    let size = menu_button_size(ui, 3, button_height);
                    ui.horizontal(|ui| {
                        if ui.add_sized(size, egui::Button::new("New game")).clicked() {
                            game.reset();
                            interaction.reset();
                            clock.reset();
                            cpu.delay_remaining = CPU_DELAY_SECONDS;
                            scene_sync.cancel_capture(&mut fx_pause);
                            scene_sync.undo_used = false;
                        }

                        let undo_clicked = ui
                            .add_enabled_ui(scene_sync.can_undo(&game), |ui| {
                                ui.add_sized(size, egui::Button::new("Undo"))
                            })
                            .inner
                            .clicked();
                        if undo_clicked {
                            perform_undo(
                                &mut game,
                                &mut interaction,
                                &mut clock,
                                &mut cpu,
                                &mut scene_sync,
                                &mut fx_pause,
                            );
                        }

                        if ui.add_sized(size, egui::Button::new("Flip")).clicked() {
                            camera.flipped = !camera.flipped;
                            scene_sync.dirty = true;
                        }
                    });

                    let size = menu_button_size(ui, 2, button_height);
                    ui.horizontal(|ui| {
                        if ui
                            .add_sized(
                                size,
                                egui::Button::new(if cpu.enabled { "CPU: On" } else { "CPU: Off" })
                                    .selected(cpu.enabled),
                            )
                            .clicked()
                        {
                            cpu.enabled = !cpu.enabled;
                            cpu.delay_remaining = CPU_DELAY_SECONDS;
                            interaction.clear_selection();
                            scene_sync.dirty = true;
                        }
                        if ui
                            .add_sized(
                                size,
                                egui::Button::new(format!("CPU: {}", color_name(cpu.color))),
                            )
                            .clicked()
                        {
                            cpu.color = match cpu.color {
                                ChessColor::White => ChessColor::Black,
                                ChessColor::Black => ChessColor::White,
                            };
                            cpu.delay_remaining = CPU_DELAY_SECONDS;
                            interaction.clear_selection();
                            scene_sync.dirty = true;
                        }
                    });

                    ui.separator();
                    ui.label("Camera");
                    let size = menu_button_size(ui, 3, button_height);
                    ui.horizontal(|ui| {
                        if ui.add_sized(size, egui::Button::new("Zoom −")).clicked() {
                            camera.zoom(-1.0);
                        }
                        if ui.add_sized(size, egui::Button::new("Zoom +")).clicked() {
                            camera.zoom(1.0);
                        }
                        if ui
                            .add_sized(size, egui::Button::new("Reset view"))
                            .clicked()
                        {
                            *camera = CameraRig::default();
                        }
                    });
                    ui.small("Drag the board to rotate / tilt · Scroll to zoom");

                    ui.horizontal(|ui| {
                        ui.checkbox(&mut battle.animations, "Battle animations");
                        if ui
                            .checkbox(&mut interaction.show_legal_moves, "Legal moves")
                            .changed()
                        {
                            scene_sync.dirty = true;
                        }
                    });

                    ui.collapsing("Chess clock", |ui| {
                        if ui.checkbox(&mut clock.enabled, "Timed chess").changed() {
                            if clock.enabled {
                                clock.reset();
                            } else {
                                clock.timed_out = None;
                            }
                        }

                        let size = menu_button_size(ui, 3, button_height);
                        ui.horizontal(|ui| {
                            if ui.add_sized(size, egui::Button::new("1 min")).clicked() {
                                clock.set_minutes(1.0);
                            }
                            if ui.add_sized(size, egui::Button::new("5 min")).clicked() {
                                clock.set_minutes(5.0);
                            }
                            if ui.add_sized(size, egui::Button::new("10 min")).clicked() {
                                clock.set_minutes(10.0);
                            }
                        });

                        let mut custom_minutes = clock.initial_seconds / 60.0;
                        ui.horizontal(|ui| {
                            ui.label("Custom");
                            if ui
                                .add(
                                    egui::DragValue::new(&mut custom_minutes)
                                        .range(0.25..=180.0)
                                        .speed(0.25)
                                        .suffix(" min"),
                                )
                                .changed()
                            {
                                clock.set_minutes(custom_minutes);
                            }
                        });

                        ui.monospace(format!(
                            "White {}   Black {}",
                            format_clock(clock.white_seconds),
                            format_clock(clock.black_seconds)
                        ));
                    });

                    ui.collapsing("Appearance", |ui| {
                        let previous_board = appearance.board_preset;
                        egui::ComboBox::from_label("Board")
                            .selected_text(format!("{:?}", appearance.board_preset))
                            .show_ui(ui, |ui| {
                                ui.selectable_value(
                                    &mut appearance.board_preset,
                                    BoardPreset::Walnut,
                                    "Walnut",
                                );
                                ui.selectable_value(
                                    &mut appearance.board_preset,
                                    BoardPreset::Marble,
                                    "Marble",
                                );
                                ui.selectable_value(
                                    &mut appearance.board_preset,
                                    BoardPreset::Tournament,
                                    "Tournament",
                                );
                                ui.selectable_value(
                                    &mut appearance.board_preset,
                                    BoardPreset::Slate,
                                    "Slate",
                                );
                                ui.selectable_value(
                                    &mut appearance.board_preset,
                                    BoardPreset::Obsidian,
                                    "Obsidian",
                                );
                                ui.selectable_value(
                                    &mut appearance.board_preset,
                                    BoardPreset::Neon,
                                    "Neon",
                                );
                            });
                        if previous_board != appearance.board_preset {
                            appearance.apply_board_preset();
                        }

                        let previous_piece = appearance.piece_preset;
                        egui::ComboBox::from_label("Pieces")
                            .selected_text(format!("{:?}", appearance.piece_preset))
                            .show_ui(ui, |ui| {
                                ui.selectable_value(
                                    &mut appearance.piece_preset,
                                    PiecePreset::Ivory,
                                    "Ivory & ebony",
                                );
                                ui.selectable_value(
                                    &mut appearance.piece_preset,
                                    PiecePreset::Walnut,
                                    "Carved wood",
                                );
                                ui.selectable_value(
                                    &mut appearance.piece_preset,
                                    PiecePreset::Brass,
                                    "Brass & gunmetal",
                                );
                                ui.selectable_value(
                                    &mut appearance.piece_preset,
                                    PiecePreset::Chrome,
                                    "Chrome",
                                );
                                ui.selectable_value(
                                    &mut appearance.piece_preset,
                                    PiecePreset::Glass,
                                    "Glass",
                                );
                                ui.selectable_value(
                                    &mut appearance.piece_preset,
                                    PiecePreset::Neon,
                                    "Neon",
                                );
                            });
                        if previous_piece != appearance.piece_preset {
                            appearance.apply_piece_preset();
                        }

                        ui.checkbox(&mut appearance.glossy, "Gloss finish");

                        ui.label("Light square");
                        ui.color_edit_button_rgb(&mut appearance.light_square);
                        ui.label("Dark square");
                        ui.color_edit_button_rgb(&mut appearance.dark_square);
                        ui.label("White pieces");
                        ui.color_edit_button_rgb(&mut appearance.white_piece);
                        ui.label("Black pieces");
                        ui.color_edit_button_rgb(&mut appearance.black_piece);
                        ui.label("Background");
                        ui.color_edit_button_rgb(&mut appearance.background);
                        ui.label("Scene light");
                        ui.color_edit_button_rgb(&mut appearance.light_color);

                        let previous_orientation = appearance.knight_orientation;
                        egui::ComboBox::from_label("Knight orientation")
                            .selected_text(format!("{}°", appearance.knight_orientation))
                            .show_ui(ui, |ui| {
                                for degrees in [0, 90, 180, 270] {
                                    ui.selectable_value(
                                        &mut appearance.knight_orientation,
                                        degrees,
                                        format!("{degrees}°"),
                                    );
                                }
                            });
                        if previous_orientation != appearance.knight_orientation {
                            scene_sync.dirty = true;
                        }

                        if ui
                            .checkbox(&mut appearance.show_captured, "Show captured pieces")
                            .changed()
                        {
                            scene_sync.dirty = true;
                        }
                        if ui
                            .checkbox(&mut appearance.captured_upright, "Captured upright")
                            .changed()
                        {
                            scene_sync.dirty = true;
                        }
                    });

                    ui.separator();
                    ui.heading(format!("Move history ({})", game.history_len()));
                    egui::ScrollArea::vertical()
                        .id_salt("move-history")
                        .max_height((ctx.viewport_rect().height() * 0.24).clamp(120.0, 300.0))
                        .stick_to_bottom(true)
                        .show(ui, |ui| {
                            for line in game.formatted_history() {
                                ui.monospace(line);
                            }
                        });

                    ui.separator();
                    ui.small("Mouse: click piece then destination");
                    ui.small("Arrows: move cursor · Enter: select · Esc: cancel");
                    ui.small("Q/E: orbit camera · F: flip · Z/X: zoom");
                });
        });

    if let Some(candidates) = interaction.pending_promotion.clone() {
        let mut chosen = None;
        egui::Window::new("Choose promotion")
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                for (label, role) in [
                    ("Queen", Role::Queen),
                    ("Rook", Role::Rook),
                    ("Bishop", Role::Bishop),
                    ("Knight", Role::Knight),
                ] {
                    if ui.button(label).clicked() {
                        chosen = candidates
                            .iter()
                            .find(|mv| (**mv).promotion() == Some(role))
                            .copied();
                    }
                }
            });

        if let Some(mv) = chosen {
            interaction.pending_promotion = None;
            commit_move(
                &mut game,
                mv,
                &mut interaction,
                &mut cpu,
                &clock,
                &battle,
                &mut scene_sync,
                &mut fx_pause,
            );
        }
    }

    ui_capture.pointer = ctx.egui_wants_pointer_input();
    ui_capture.keyboard = ctx.egui_wants_keyboard_input();
    Ok(())
}

// ============================================================
// Input and game flow
// ============================================================

fn camera_pointer_system(
    mouse: Res<ButtonInput<MouseButton>>,
    scroll: Res<AccumulatedMouseScroll>,
    windows: Query<&Window, With<PrimaryWindow>>,
    ui_capture: Res<UiCapture>,
    mut gesture: ResMut<CameraGesture>,
    mut rig: ResMut<CameraRig>,
) {
    let Some(window) = windows.iter().next() else {
        return;
    };
    let cursor = window.cursor_position().filter(|_| window.focused);
    let over_game =
        cursor.is_some_and(|position| position.x < ui_capture.game_right) && !ui_capture.pointer;
    let delta = gesture.update(
        cursor,
        mouse.pressed(MouseButton::Left),
        mouse.just_pressed(MouseButton::Left),
        mouse.just_released(MouseButton::Left),
        over_game,
    );
    rig.drag(delta);
    if over_game {
        let amount = match scroll.unit {
            MouseScrollUnit::Line => scroll.delta.y,
            MouseScrollUnit::Pixel => scroll.delta.y / 100.0,
        };
        rig.zoom(amount);
    }
}

#[allow(clippy::too_many_arguments)]
fn board_input_system(
    gesture: Res<CameraGesture>,
    keys: Res<ButtonInput<KeyCode>>,
    cameras: Query<(&Camera, &GlobalTransform), With<MainCamera>>,
    ui_capture: Res<UiCapture>,
    mut game: ResMut<BattleGame>,
    mut interaction: ResMut<InteractionState>,
    mut cpu: ResMut<CpuSettings>,
    clock: Res<ChessClock>,
    battle: Res<BattleSettings>,
    camera_rig: Res<CameraRig>,
    mut scene_sync: ResMut<SceneSync>,
    mut fx_pause: ResMut<FxPause>,
) {
    if interaction.pending_promotion.is_some()
        || scene_sync.pending_move.is_some()
        || game.outcome().is_some()
        || clock.timed_out.is_some()
        || (cpu.enabled && game.side_to_move() == cpu.color)
    {
        return;
    }

    if !ui_capture.keyboard {
        let mut file_delta = 0;
        let mut rank_delta = 0;
        if keys.just_pressed(KeyCode::ArrowLeft) {
            file_delta = if camera_rig.flipped { 1 } else { -1 };
        }
        if keys.just_pressed(KeyCode::ArrowRight) {
            file_delta = if camera_rig.flipped { -1 } else { 1 };
        }
        if keys.just_pressed(KeyCode::ArrowUp) {
            rank_delta = if camera_rig.flipped { -1 } else { 1 };
        }
        if keys.just_pressed(KeyCode::ArrowDown) {
            rank_delta = if camera_rig.flipped { 1 } else { -1 };
        }

        if file_delta != 0 || rank_delta != 0 {
            let (file, rank) = square_indices(interaction.keyboard_cursor);
            let next_file = (file as i32 + file_delta).clamp(0, 7) as usize;
            let next_rank = (rank as i32 + rank_delta).clamp(0, 7) as usize;
            interaction.keyboard_cursor = square_from_indices(next_file, next_rank);
            scene_sync.dirty = true;
        }

        if keys.just_pressed(KeyCode::Escape) {
            interaction.clear_selection();
            scene_sync.dirty = true;
        }

        if keys.just_pressed(KeyCode::Enter) || keys.just_pressed(KeyCode::Space) {
            let square = interaction.keyboard_cursor;
            handle_square_action(
                square,
                &mut game,
                &mut interaction,
                &mut cpu,
                &clock,
                &battle,
                &mut scene_sync,
                &mut fx_pause,
            );
        }
    }

    if ui_capture.pointer {
        return;
    }

    let Some(cursor) = gesture.click else {
        return;
    };
    let Some((camera, camera_transform)) = cameras.iter().next() else {
        return;
    };
    let Ok(ray) = camera.viewport_to_world(camera_transform, cursor) else {
        return;
    };
    let Some(point) = ray.plane_intersection_point(Vec3::ZERO, InfinitePlane3d::new(Vec3::Y))
    else {
        return;
    };
    let Some(square) = square_from_world(point) else {
        return;
    };

    interaction.keyboard_cursor = square;
    handle_square_action(
        square,
        &mut game,
        &mut interaction,
        &mut cpu,
        &clock,
        &battle,
        &mut scene_sync,
        &mut fx_pause,
    );
}

#[allow(clippy::too_many_arguments)]
fn handle_square_action(
    square: Square,
    game: &mut BattleGame,
    interaction: &mut InteractionState,
    cpu: &mut CpuSettings,
    clock: &ChessClock,
    battle: &BattleSettings,
    scene_sync: &mut SceneSync,
    fx_pause: &mut FxPause,
) {
    if clock.timed_out.is_some() || game.outcome().is_some() || scene_sync.pending_move.is_some() {
        return;
    }

    if Some(square) == interaction.selected {
        interaction.clear_selection();
        scene_sync.dirty = true;
        return;
    }

    if let Some(piece) = game.piece_at(square)
        && piece.color == game.side_to_move()
    {
        select_square(square, game, interaction);
        scene_sync.dirty = true;
        return;
    }

    let Some(from) = interaction.selected else {
        return;
    };
    let candidates = game.legal_moves_between(from, square);
    if candidates.is_empty() {
        return;
    }

    if candidates.len() > 1 {
        interaction.pending_promotion = Some(candidates);
        return;
    }

    if let Some(mv) = candidates.into_iter().next() {
        commit_move(
            game,
            mv,
            interaction,
            cpu,
            clock,
            battle,
            scene_sync,
            fx_pause,
        );
    }
}

fn select_square(square: Square, game: &BattleGame, interaction: &mut InteractionState) {
    interaction.selected = Some(square);
    interaction.legal_destinations.clear();
    for mv in game.legal_moves_from(square) {
        if let Some(target) = display_move_to(&mv)
            && !interaction.legal_destinations.contains(&target)
        {
            interaction.legal_destinations.push(target);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn commit_move(
    game: &mut BattleGame,
    mv: Move,
    interaction: &mut InteractionState,
    cpu: &mut CpuSettings,
    clock: &ChessClock,
    battle: &BattleSettings,
    scene_sync: &mut SceneSync,
    fx_pause: &mut FxPause,
) {
    if clock.timed_out.is_some()
        || scene_sync.pending_move.is_some()
        || !game.position().is_legal(mv)
    {
        return;
    }

    let fx_request = if battle.animations {
        match (move_from(&mv), capture_square(&mv)) {
            (Some(from), Some(target)) => Some(CaptureFxRequest {
                from,
                target,
                role: mv.role(),
            }),
            _ => None,
        }
    } else {
        None
    };

    // Keep the original position visible until the projectile reaches its victim.
    if fx_request.is_some() || game.play(mv).is_ok() {
        scene_sync.undo_used = false;
        interaction.clear_selection();
        interaction.pending_promotion = None;
        cpu.delay_remaining = CPU_DELAY_SECONDS;
        scene_sync.pending_fx = fx_request;
        scene_sync.dirty = true;
        if fx_request.is_some() {
            scene_sync.pending_move = Some(mv);
            fx_pause.remaining = 0.78;
        }
    }
}

fn perform_undo(
    game: &mut BattleGame,
    interaction: &mut InteractionState,
    clock: &mut ChessClock,
    cpu: &mut CpuSettings,
    scene_sync: &mut SceneSync,
    fx_pause: &mut FxPause,
) {
    if !scene_sync.can_undo(game) {
        return;
    }

    // A pending capture is the latest move: cancel it without rewinding history.
    // Completed moves are undone one at a time, including in CPU mode.
    if scene_sync.pending_move.is_none() && game.undo().is_none() {
        return;
    }
    scene_sync.undo_used = true;

    interaction.clear_selection();
    interaction.pending_promotion = None;
    clock.timed_out = None;
    cpu.delay_remaining = CPU_DELAY_SECONDS;
    scene_sync.cancel_capture(fx_pause);
}

// ============================================================
// CPU and clocks
// ============================================================

#[allow(clippy::too_many_arguments)]
fn cpu_turn_system(
    time: Res<Time>,
    mut game: ResMut<BattleGame>,
    mut cpu: ResMut<CpuSettings>,
    clock: Res<ChessClock>,
    battle: Res<BattleSettings>,
    mut interaction: ResMut<InteractionState>,
    mut scene_sync: ResMut<SceneSync>,
    mut fx_pause: ResMut<FxPause>,
) {
    let cpu_turn = cpu.enabled
        && game.side_to_move() == cpu.color
        && game.outcome().is_none()
        && clock.timed_out.is_none()
        && interaction.pending_promotion.is_none();

    if !cpu_turn || fx_pause.remaining > 0.0 || scene_sync.pending_move.is_some() {
        cpu.delay_remaining = CPU_DELAY_SECONDS;
        return;
    }

    cpu.delay_remaining -= time.delta_secs();
    if cpu.delay_remaining > 0.0 {
        return;
    }

    let Some(mv) = choose_cpu_move(game.position(), cpu.color) else {
        return;
    };
    commit_move(
        &mut game,
        mv,
        &mut interaction,
        &mut cpu,
        &clock,
        &battle,
        &mut scene_sync,
        &mut fx_pause,
    );
}

fn clock_system(
    time: Res<Time>,
    game: Res<BattleGame>,
    mut clock: ResMut<ChessClock>,
    mut interaction: ResMut<InteractionState>,
    fx_pause: Res<FxPause>,
    scene_sync: Res<SceneSync>,
) {
    if !clock.enabled
        || !game.has_started()
        || game.outcome().is_some()
        || clock.timed_out.is_some()
        || fx_pause.remaining > 0.0
        || scene_sync.pending_move.is_some()
    {
        return;
    }

    let side = game.side_to_move();
    let remaining = clock.remaining_mut(side);
    *remaining = (*remaining - time.delta_secs()).max(0.0);
    if *remaining <= 0.0 {
        clock.timed_out = Some(side);
        interaction.clear_selection();
    }
}

// ============================================================
// Scene synchronization and appearance
// ============================================================

fn appearance_system(
    appearance: Res<Appearance>,
    handles: Option<Res<SceneHandles>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut camera_query: Query<&mut Camera, With<MainCamera>>,
    mut light_query: Query<&mut DirectionalLight, With<KeyLight>>,
) {
    if !appearance.is_changed() {
        return;
    }
    let Some(handles) = handles else {
        return;
    };

    if let Some(mut material) = materials.get_mut(&handles.light_square) {
        material.base_color = rgb(appearance.light_square);
        material.perceptual_roughness = appearance.board_roughness;
        material.metallic = appearance.board_metallic;
    }
    if let Some(mut material) = materials.get_mut(&handles.dark_square) {
        material.base_color = rgb(appearance.dark_square);
        material.perceptual_roughness = appearance.board_roughness;
        material.metallic = appearance.board_metallic;
    }
    if let Some(mut material) = materials.get_mut(&handles.white_piece) {
        *material = piece_material(
            rgb(appearance.white_piece),
            appearance.piece_roughness,
            appearance.piece_metallic,
            appearance.glossy,
        );
    }
    if let Some(mut material) = materials.get_mut(&handles.black_piece) {
        *material = piece_material(
            rgb(appearance.black_piece),
            appearance.piece_roughness,
            appearance.piece_metallic,
            appearance.glossy,
        );
    }
    for mut camera in &mut camera_query {
        camera.clear_color = rgb(appearance.background).into();
    }
    for mut light in &mut light_query {
        light.color = rgb(appearance.light_color);
    }
}

#[allow(clippy::too_many_arguments)]
fn sync_scene_system(
    mut commands: Commands,
    game: Res<BattleGame>,
    interaction: Res<InteractionState>,
    appearance: Res<Appearance>,
    handles: Option<Res<SceneHandles>>,
    mut scene_sync: ResMut<SceneSync>,
    piece_query: Query<Entity, With<PieceVisual>>,
    highlight_query: Query<Entity, With<HighlightVisual>>,
) {
    if !scene_sync.dirty {
        return;
    }
    let Some(handles) = handles else {
        return;
    };

    for entity in &piece_query {
        commands.entity(entity).despawn();
    }
    for entity in &highlight_query {
        commands.entity(entity).despawn();
    }

    for rank in 0..8 {
        for file in 0..8 {
            let square = square_from_indices(file, rank);
            if let Some(piece) = game.piece_at(square) {
                spawn_piece_visual(
                    &mut commands,
                    &handles,
                    piece,
                    square_world(square),
                    0.68,
                    false,
                    appearance.knight_orientation,
                );
            }
        }
    }

    if appearance.show_captured {
        let mut white_count = 0usize;
        let mut black_count = 0usize;
        for captured in game.captured_pieces() {
            let index = match captured.captured_by {
                ChessColor::White => {
                    let current = white_count;
                    white_count += 1;
                    current
                }
                ChessColor::Black => {
                    let current = black_count;
                    black_count += 1;
                    current
                }
            };
            let col = index % 8;
            let row = index / 8;
            let x = -2.75 + col as f32 * 0.78;
            let z = match captured.captured_by {
                ChessColor::White => 5.10 + row as f32 * 0.72,
                ChessColor::Black => -5.10 - row as f32 * 0.72,
            };
            spawn_piece_visual(
                &mut commands,
                &handles,
                captured.piece,
                Vec3::new(x, 0.03, z),
                0.42,
                !appearance.captured_upright,
                appearance.knight_orientation,
            );
        }
    }

    if let Some(selected) = interaction.selected {
        spawn_highlight(
            &mut commands,
            &handles,
            selected,
            handles.selected.clone(),
            0.15,
        );

        if interaction.show_legal_moves {
            for target in &interaction.legal_destinations {
                let capture = game
                    .legal_moves_between(selected, *target)
                    .iter()
                    .any(|mv| (*mv).is_capture());
                spawn_highlight(
                    &mut commands,
                    &handles,
                    *target,
                    if capture {
                        handles.capture.clone()
                    } else {
                        handles.legal.clone()
                    },
                    0.14,
                );
            }
        }
    }

    spawn_highlight(
        &mut commands,
        &handles,
        interaction.keyboard_cursor,
        handles.cursor.clone(),
        0.13,
    );

    if let Some((from, to)) = game.last_move_squares()
        && interaction.selected.is_none()
    {
        spawn_highlight(&mut commands, &handles, from, handles.cursor.clone(), 0.125);
        spawn_highlight(&mut commands, &handles, to, handles.cursor.clone(), 0.125);
    }

    if let Some(request) = scene_sync.pending_fx.take() {
        spawn_projectile(&mut commands, &handles, request);
    }

    scene_sync.dirty = false;
}

fn spawn_piece_visual(
    commands: &mut Commands,
    handles: &SceneHandles,
    piece: ChessPiece,
    world_position: Vec3,
    scale: f32,
    upside_down: bool,
    knight_orientation: i32,
) {
    let material = match piece.color {
        ChessColor::White => handles.white_piece.clone(),
        ChessColor::Black => handles.black_piece.clone(),
    };

    let mut transform = Transform::from_translation(world_position).with_scale(Vec3::splat(scale));
    let knight_rotation = if piece.role == Role::Knight {
        (knight_orientation as f32).to_radians()
    } else {
        0.0
    };
    transform.rotation = Quat::from_rotation_x(if upside_down { PI } else { 0.0 })
        * Quat::from_rotation_y(knight_rotation);

    commands
        .spawn((transform, PieceVisual))
        .with_children(|parent| {
            let mut part = |position: Vec3, part_scale: Vec3| {
                parent.spawn((
                    Mesh3d(handles.cube.clone()),
                    MeshMaterial3d(material.clone()),
                    Transform::from_translation(position).with_scale(part_scale),
                ));
            };

            part(Vec3::new(0.0, 0.08, 0.0), Vec3::new(0.72, 0.16, 0.72));
            part(Vec3::new(-0.16, 0.34, 0.0), Vec3::new(0.17, 0.42, 0.20));
            part(Vec3::new(0.16, 0.34, 0.0), Vec3::new(0.17, 0.42, 0.20));
            part(Vec3::new(0.0, 0.70, 0.0), Vec3::new(0.46, 0.44, 0.32));

            match piece.role {
                Role::Pawn => {
                    part(Vec3::new(0.0, 1.02, 0.0), Vec3::new(0.32, 0.30, 0.32));
                }
                Role::Knight => {
                    part(Vec3::new(0.0, 1.02, -0.02), Vec3::new(0.33, 0.34, 0.30));
                    part(Vec3::new(0.0, 1.05, -0.25), Vec3::new(0.24, 0.18, 0.32));
                    part(Vec3::new(-0.14, 1.28, 0.02), Vec3::new(0.08, 0.22, 0.08));
                    part(Vec3::new(0.14, 1.28, 0.02), Vec3::new(0.08, 0.22, 0.08));
                }
                Role::Bishop => {
                    part(Vec3::new(0.0, 1.00, 0.0), Vec3::new(0.28, 0.25, 0.28));
                    part(Vec3::new(0.0, 1.07, 0.0), Vec3::new(0.52, 0.07, 0.32));
                    parent.spawn((
                        Mesh3d(handles.bishop_mitre.clone()),
                        MeshMaterial3d(material.clone()),
                        Transform::from_xyz(0.0, 1.08, 0.0),
                    ));
                }
                Role::Rook => {
                    part(Vec3::new(0.0, 1.05, 0.0), Vec3::new(0.38, 0.30, 0.34));
                    part(Vec3::new(-0.27, 1.29, 0.0), Vec3::new(0.14, 0.18, 0.40));
                    part(Vec3::new(0.27, 1.29, 0.0), Vec3::new(0.14, 0.18, 0.40));
                    part(Vec3::new(0.0, 1.29, 0.27), Vec3::new(0.40, 0.18, 0.14));
                    part(Vec3::new(0.0, 1.29, -0.27), Vec3::new(0.40, 0.18, 0.14));
                }
                Role::Queen => {
                    part(Vec3::new(0.0, 1.01, 0.0), Vec3::new(0.32, 0.28, 0.32));
                    parent.spawn((
                        Mesh3d(handles.queen_circlet.clone()),
                        MeshMaterial3d(material.clone()),
                        Transform::from_xyz(0.0, 1.18, 0.0),
                    ));
                    // An open, eight-point crown reads clearly from every orbit angle.
                    for index in 0..8 {
                        let angle = index as f32 * PI / 4.0;
                        let radial = Vec3::new(angle.cos(), 0.0, angle.sin());
                        let base = radial * 0.24 + Vec3::Y * 1.18;
                        let tip = radial * 0.33 + Vec3::Y * 1.47;
                        let stem = tip - base;
                        parent.spawn((
                            Mesh3d(handles.cube.clone()),
                            MeshMaterial3d(material.clone()),
                            Transform::from_translation((base + tip) * 0.5)
                                .with_rotation(Quat::from_rotation_arc(Vec3::Y, stem.normalize()))
                                .with_scale(Vec3::new(0.065, stem.length(), 0.065)),
                        ));
                        parent.spawn((
                            Mesh3d(handles.sphere.clone()),
                            MeshMaterial3d(material.clone()),
                            Transform::from_translation(tip).with_scale(Vec3::splat(0.11)),
                        ));
                    }
                }
                Role::King => {
                    part(Vec3::new(0.0, 1.06, 0.0), Vec3::new(0.36, 0.34, 0.34));
                    part(Vec3::new(0.0, 1.42, 0.0), Vec3::new(0.10, 0.34, 0.10));
                    part(Vec3::new(0.0, 1.48, 0.0), Vec3::new(0.36, 0.10, 0.10));
                }
            }
        });
}

fn bishop_mitre_mesh() -> Mesh {
    // A pointed silhouette with a real diagonal cut, extruded through the hat.
    // The left edge joins the tip to the base, so the notch is open on one side.
    let outline = [
        Vec2::new(-0.25, 0.0),
        Vec2::new(0.25, 0.0),
        Vec2::new(0.12, 0.30),
        Vec2::new(-0.04, 0.21),
        Vec2::new(-0.08, 0.27),
        Vec2::new(0.09, 0.37),
        Vec2::new(0.0, 0.60),
    ];
    let faces = [[0, 1, 3], [1, 2, 3], [0, 3, 4], [0, 4, 6], [4, 5, 6]];
    let vertex = |index: usize, z: f32| Vec3::new(outline[index].x, outline[index].y * 0.8, z);
    let mut positions = Vec::new();
    let mut normals = Vec::new();
    let mut triangle = |a: Vec3, b: Vec3, c: Vec3| {
        let normal = (b - a).cross(c - a).normalize().to_array();
        positions.extend([a.to_array(), b.to_array(), c.to_array()]);
        normals.extend([normal; 3]);
    };
    for [a, b, c] in faces {
        triangle(vertex(a, 0.14), vertex(b, 0.14), vertex(c, 0.14));
        triangle(vertex(c, -0.14), vertex(b, -0.14), vertex(a, -0.14));
    }
    for a in 0..outline.len() {
        let b = (a + 1) % outline.len();
        triangle(vertex(a, -0.14), vertex(b, -0.14), vertex(b, 0.14));
        triangle(vertex(a, -0.14), vertex(b, 0.14), vertex(a, 0.14));
    }
    let uvs = vec![[0.0, 0.0]; positions.len()];
    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
}

fn spawn_highlight(
    commands: &mut Commands,
    handles: &SceneHandles,
    square: Square,
    material: Handle<StandardMaterial>,
    y: f32,
) {
    let mut position = square_world(square);
    position.y = y;
    commands.spawn((
        Mesh3d(handles.cube.clone()),
        MeshMaterial3d(material),
        Transform::from_translation(position).with_scale(Vec3::new(0.88, 0.035, 0.88)),
        HighlightVisual,
    ));
}

fn spawn_projectile(commands: &mut Commands, handles: &SceneHandles, request: CaptureFxRequest) {
    let start = square_world(request.from) + Vec3::Y * 0.78;
    let end = square_world(request.target) + Vec3::Y * 0.56;
    let profile = projectile_profile(request.role);
    let (mesh, shape) = match request.role {
        Role::Rook | Role::King => (handles.sphere.clone(), Vec3::ONE),
        Role::Knight => (handles.cube.clone(), Vec3::new(1.0, 1.8, 1.0)),
        Role::Bishop | Role::Queen => (handles.cube.clone(), Vec3::new(0.65, 2.4, 0.65)),
        Role::Pawn => (handles.cube.clone(), Vec3::ONE),
    };

    for index in 0..profile.count {
        commands.spawn((
            Mesh3d(mesh.clone()),
            MeshMaterial3d(handles.attack_material(request.role)),
            Transform::from_translation(start).with_scale(shape * profile.size),
            if index == 0 {
                Visibility::Visible
            } else {
                Visibility::Hidden
            },
            Projectile {
                start,
                end,
                elapsed: -(index as f32) * profile.interval,
                duration: profile.duration,
                arc: profile.arc,
                role: request.role,
                index,
            },
        ));
    }
}

struct ProjectileProfile {
    count: usize,
    color: u32,
    size: f32,
    arc: f32,
    duration: f32,
    interval: f32,
}

fn projectile_profile(role: Role) -> ProjectileProfile {
    let (count, color, size, arc, duration, interval) = match role {
        Role::Pawn => (1, 0xff7a24, 0.13, 0.55, 0.34, 0.0),
        Role::Knight => (2, 0x28bcff, 0.14, 1.10, 0.50, 0.12),
        Role::Bishop => (3, 0x9755ff, 0.12, 0.85, 0.58, 0.10),
        Role::Rook => (4, 0xff3038, 0.27, 0.35, 0.46, 0.16),
        Role::Queen => (8, 0xff48cb, 0.14, 1.10, 0.65, 0.07),
        Role::King => (12, 0xffd23f, 0.22, 1.90, 0.75, 0.055),
    };
    ProjectileProfile {
        count,
        color,
        size,
        arc,
        duration,
        interval,
    }
}

fn create_attack_materials(
    materials: &mut Assets<StandardMaterial>,
) -> [Handle<StandardMaterial>; 6] {
    ATTACK_ROLES.map(|role| {
        materials.add(StandardMaterial {
            base_color: rgb(hex_rgb(projectile_profile(role).color)),
            unlit: true,
            ..default()
        })
    })
}

impl Projectile {
    fn position(&self, t: f32) -> Vec3 {
        let eased = t * t * (3.0 - 2.0 * t);
        let envelope = (PI * t).sin();
        let direction = (self.end - self.start).normalize_or_zero();
        let sideways = Vec3::new(-direction.z, 0.0, direction.x).normalize_or_zero();
        let phase = self.index as f32 * 2.0 * PI / projectile_profile(self.role).count as f32;
        let (lateral, vertical) = match self.role {
            Role::Pawn => (0.0, 0.0), // A single arcing spark.
            Role::Knight => ((2.0 * PI * t + phase).sin() * 0.75, 0.0), // Crossing lances.
            Role::Bishop => (
                (6.0 * PI * t + phase).sin() * 0.45,
                (6.0 * PI * t + phase).cos() * 0.45,
            ), // Helical bolts.
            Role::Rook => (0.0, (self.index % 2) as f32 * 0.12), // Low cannon barrage.
            Role::Queen => (
                (self.index as f32 / 7.0 - 0.5) * 3.0,
                (phase * 2.0).cos() * 0.35,
            ), // Spreading crystal fan.
            Role::King => (phase.sin() * 1.25, phase.cos() * 0.65), // Lofted crown volley.
        };
        self.start.lerp(self.end, eased)
            + (Vec3::Y * (self.arc + vertical) + sideways * lateral) * envelope
    }
}

// ============================================================
// Animation and camera
// ============================================================

#[allow(clippy::too_many_arguments)]
fn capture_fx_system(
    mut commands: Commands,
    time: Res<Time>,
    handles: Option<Res<SceneHandles>>,
    mut projectiles: Query<(Entity, &mut Projectile, &mut Transform, &mut Visibility)>,
    mut debris_query: Query<(Entity, &mut Debris, &mut Transform), Without<Projectile>>,
    mut fx_pause: ResMut<FxPause>,
    mut game: ResMut<BattleGame>,
    mut scene_sync: ResMut<SceneSync>,
) {
    if scene_sync.clear_fx {
        for (entity, _, _, _) in &projectiles {
            commands.entity(entity).despawn();
        }
        for (entity, _, _) in &debris_query {
            commands.entity(entity).despawn();
        }
        scene_sync.clear_fx = false;
        return;
    }
    fx_pause.remaining = (fx_pause.remaining - time.delta_secs()).max(0.0);
    let Some(handles) = handles else {
        return;
    };

    let mut explosion_active = false;
    let mut projectiles_active = false;
    for (entity, mut projectile, mut transform, mut visibility) in &mut projectiles {
        projectile.elapsed += time.delta_secs();
        if projectile.elapsed < 0.0 {
            projectiles_active = true;
            continue;
        }
        *visibility = Visibility::Visible;
        let t = (projectile.elapsed / projectile.duration).clamp(0.0, 1.0);
        projectiles_active |= t < 1.0;
        transform.translation = projectile.position(t);
        transform.rotate_y(time.delta_secs() * 10.0);
        transform.rotate_x(time.delta_secs() * 7.0);

        if t >= 1.0 {
            commands.entity(entity).despawn();
            scene_sync.exploding = true;
            explosion_active = true;
            let directions = [
                Vec3::new(1.0, 1.8, 0.0),
                Vec3::new(-1.0, 1.7, 0.2),
                Vec3::new(0.2, 2.0, 1.0),
                Vec3::new(-0.2, 1.9, -1.0),
                Vec3::new(0.8, 1.5, 0.8),
                Vec3::new(-0.8, 1.6, 0.8),
                Vec3::new(0.8, 1.6, -0.8),
                Vec3::new(-0.8, 1.5, -0.8),
                Vec3::new(0.4, 2.2, 0.1),
                Vec3::new(-0.4, 2.1, -0.1),
                Vec3::new(0.1, 1.8, 0.5),
                Vec3::new(-0.1, 1.8, -0.5),
            ];
            for (index, direction) in directions.into_iter().enumerate() {
                let speed = 1.3 + (index % 4) as f32 * 0.18;
                commands.spawn((
                    Mesh3d(handles.cube.clone()),
                    MeshMaterial3d(handles.attack_material(projectile.role)),
                    Transform::from_translation(projectile.end)
                        .with_scale(Vec3::splat(0.07 + (index % 3) as f32 * 0.018)),
                    Debris {
                        velocity: direction.normalize() * speed,
                        remaining: 0.48 + (index % 3) as f32 * 0.08,
                    },
                ));
            }
        }
    }

    for (entity, mut debris, mut transform) in &mut debris_query {
        debris.remaining -= time.delta_secs();
        debris.velocity.y -= 5.8 * time.delta_secs();
        transform.translation += debris.velocity * time.delta_secs();
        transform.rotate_x(time.delta_secs() * 12.0);
        transform.rotate_z(time.delta_secs() * 9.0);
        if debris.remaining <= 0.0 {
            commands.entity(entity).despawn();
        } else {
            explosion_active = true;
        }
    }

    // The attacker and victim stay put through both the flight and explosion.
    // Commit only after the last fragment has disappeared, then rebuild the board.
    if scene_sync.exploding && !explosion_active && !projectiles_active {
        scene_sync.exploding = false;
        if let Some(mv) = scene_sync.pending_move.take() {
            if let Err(error) = game.play(mv) {
                warn!("Could not complete animated capture: {error}");
            }
            scene_sync.dirty = true;
        }
        fx_pause.remaining = 0.0;
    }
}

fn camera_system(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    ui_capture: Res<UiCapture>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut rig: ResMut<CameraRig>,
    mut cameras: Query<(&mut Transform, &Projection), With<MainCamera>>,
) {
    if !ui_capture.keyboard {
        let orbit_speed = 1.15 * time.delta_secs();
        if keys.pressed(KeyCode::KeyQ) {
            rig.orbit -= orbit_speed;
        }
        if keys.pressed(KeyCode::KeyE) {
            rig.orbit += orbit_speed;
        }
        if keys.just_pressed(KeyCode::KeyF) {
            rig.flipped = !rig.flipped;
        }
        if keys.pressed(KeyCode::KeyZ) {
            rig.zoom(4.0 * time.delta_secs());
        }
        if keys.pressed(KeyCode::KeyX) {
            rig.zoom(-4.0 * time.delta_secs());
        }
    }

    let yaw = rig.orbit + if rig.flipped { PI } else { 0.0 };
    let offset = Vec3::new(
        yaw.sin() * rig.distance * rig.elevation.cos(),
        rig.distance * rig.elevation.sin(),
        yaw.cos() * rig.distance * rig.elevation.cos(),
    );
    for (mut transform, projection) in &mut cameras {
        let mut target = Vec3::new(0.0, 0.45, 0.0);
        if let Some(window) = windows.iter().next()
            && let Projection::Perspective(perspective) = projection
        {
            // Center the board in the space beside the sidebar. Keep the camera
            // viewport full-sized because egui renders into this same camera.
            let panel_width = if ui_capture.game_right > 0.0 {
                window.width() - ui_capture.game_right
            } else {
                sidebar_dimensions(egui::vec2(window.width(), window.height())).0
            };
            let shift = panel_width / window.height().max(1.0)
                * rig.distance
                * (perspective.fov * 0.5).tan();
            target += Vec3::new(yaw.cos(), 0.0, -yaw.sin()) * shift;
        }
        *transform = Transform::from_translation(target + offset).looking_at(target, Vec3::Y);
    }
}

// ============================================================
// Helpers
// ============================================================

fn sidebar_dimensions(window: egui::Vec2) -> (f32, f32, f32) {
    (
        (window.x * 0.30).clamp(340.0, 520.0),
        (window.y * 0.052).clamp(36.0, 64.0),
        (window.y / 860.0 * 16.0).clamp(15.0, 21.0),
    )
}

fn menu_button_size(ui: &egui::Ui, columns: usize, height: f32) -> egui::Vec2 {
    let gaps = ui.spacing().item_spacing.x * (columns - 1) as f32;
    egui::vec2((ui.available_width() - gaps) / columns as f32, height)
}

fn style_sidebar(ui: &mut egui::Ui, button_height: f32, font_size: f32) {
    let style = ui.style_mut();
    style.spacing.interact_size.y = button_height;
    style.spacing.button_padding = egui::vec2(10.0, 8.0);
    style.spacing.item_spacing = egui::vec2(8.0, 8.0);
    for text_style in [egui::TextStyle::Body, egui::TextStyle::Button] {
        style
            .text_styles
            .insert(text_style, egui::FontId::proportional(font_size));
    }
}

fn board_material(color: Color, roughness: f32, metallic: f32) -> StandardMaterial {
    StandardMaterial {
        base_color: color,
        perceptual_roughness: roughness,
        metallic,
        ..default()
    }
}

fn piece_material(color: Color, roughness: f32, metallic: f32, glossy: bool) -> StandardMaterial {
    StandardMaterial {
        base_color: color,
        perceptual_roughness: if glossy {
            (roughness * 0.42).max(0.08)
        } else {
            (roughness + 0.16).min(0.95)
        },
        metallic,
        reflectance: if glossy { 0.75 } else { 0.25 },
        ..default()
    }
}

const fn hex_rgb(value: u32) -> [f32; 3] {
    [
        ((value >> 16) & 0xff) as f32 / 255.0,
        ((value >> 8) & 0xff) as f32 / 255.0,
        (value & 0xff) as f32 / 255.0,
    ]
}

fn rgb(value: [f32; 3]) -> Color {
    Color::srgb(value[0], value[1], value[2])
}

fn square_world(square: Square) -> Vec3 {
    let (file, rank) = square_indices(square);
    Vec3::new(file as f32 - 3.5, 0.10, 3.5 - rank as f32)
}

fn square_from_world(point: Vec3) -> Option<Square> {
    if point.x < -4.0 || point.x >= 4.0 || point.z <= -4.0 || point.z > 4.0 {
        return None;
    }

    let file = (point.x + 4.0).floor() as i32;
    let rank = (4.0 - point.z).floor() as i32;
    if !(0..=7).contains(&file) || !(0..=7).contains(&rank) {
        return None;
    }

    Some(square_from_indices(file as usize, rank as usize))
}

fn format_clock(seconds: f32) -> String {
    let total = seconds.max(0.0).ceil() as u32;
    format!("{:02}:{:02}", total / 60, total % 60)
}

#[cfg(test)]
mod camera_tests {
    use super::*;

    #[test]
    fn menu_buttons_resize_and_fit_the_sidebar() {
        let ctx = egui::Context::default();
        let mut previous_button_size = egui::Vec2::ZERO;
        for screen_size in [
            egui::vec2(900.0, 620.0),
            egui::vec2(1320.0, 860.0),
            egui::vec2(1920.0, 1080.0),
        ] {
            let (width, height, font) = sidebar_dimensions(screen_size);
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, screen_size)),
                ..default()
            };
            let mut rendered_size = egui::Vec2::ZERO;
            let output = ctx.run_ui(input, |root| {
                egui::Panel::right("controls")
                    .exact_size(width)
                    .resizable(false)
                    .show(root, |ui| {
                        style_sidebar(ui, height, font);
                        egui::ScrollArea::vertical().show(ui, |ui| {
                            let right = ui.max_rect().right();
                            let size = menu_button_size(ui, 3, height);
                            for row in [
                                ["New game", "Undo", "Flip"],
                                ["Zoom −", "Zoom +", "Reset view"],
                            ] {
                                ui.horizontal(|ui| {
                                    for label in row {
                                        let response = ui.add_sized(size, egui::Button::new(label));
                                        assert!(
                                            response.rect.right() <= right + 1.0,
                                            "{label} overflow at {screen_size:?}: {:?}, right {right}, requested {size:?}", response.rect
                                        );
                                        assert!(response.rect.height() >= 36.0);
                                        rendered_size = response.rect.size();
                                    }
                                });
                            }
                        });
                    });
            });
            output.drop_without_applying_deltas();
            assert!(rendered_size.x > previous_button_size.x);
            assert!(rendered_size.y > previous_button_size.y);
            previous_button_size = rendered_size;
        }
    }

    #[test]
    fn click_selects_on_release_but_drag_only_moves_camera() {
        let mut gesture = CameraGesture::default();
        let start = Vec2::new(100.0, 100.0);
        gesture.update(Some(start), true, true, false, true);
        assert!(gesture.click.is_none());
        gesture.update(Some(start + Vec2::ONE), false, false, true, true);
        assert_eq!(gesture.click, Some(start + Vec2::ONE));

        gesture.update(Some(start), true, true, false, true);
        let delta = gesture.update(
            Some(start + Vec2::new(60.0, 30.0)),
            true,
            false,
            false,
            true,
        );
        let mut camera = CameraRig::default();
        let original_elevation = camera.elevation;
        camera.drag(delta);
        assert!(camera.orbit < 0.0);
        assert!(camera.elevation > original_elevation);
        // Returning to the starting point must not turn a drag into a click.
        gesture.update(Some(start), false, false, true, true);
        assert!(gesture.click.is_none());
    }

    #[test]
    fn sidebar_and_lost_focus_do_not_trigger_board_gestures() {
        let mut gesture = CameraGesture::default();
        let cursor = Vec2::new(100.0, 100.0);
        gesture.update(Some(cursor), true, true, false, false);
        assert_eq!(
            gesture.update(Some(Vec2::ZERO), true, false, false, true),
            Vec2::ZERO
        );
        gesture.update(Some(Vec2::ZERO), false, false, true, true);
        assert!(gesture.click.is_none());

        gesture.update(Some(cursor), true, true, false, true);
        gesture.update(None, true, false, false, false);
        gesture.update(Some(cursor), false, false, true, true);
        assert!(gesture.click.is_none());
    }

    #[test]
    fn camera_zoom_and_tilt_stay_within_usable_limits() {
        let mut camera = CameraRig::default();
        let distance = camera.distance;
        camera.zoom(1.0);
        assert!(camera.distance < distance);
        camera.zoom(-1.0);
        assert!((camera.distance - distance).abs() < 0.001);
        camera.zoom(1000.0);
        assert_eq!(camera.distance, 7.0);
        camera.zoom(-1000.0);
        assert_eq!(camera.distance, 24.0);
        camera.drag(Vec2::new(0.0, 10000.0));
        assert_eq!(camera.elevation, 80.0_f32.to_radians());
        camera.drag(Vec2::new(0.0, -10000.0));
        assert_eq!(camera.elevation, 15.0_f32.to_radians());
    }
}

#[cfg(test)]
mod capture_tests {
    use super::*;
    use std::time::Duration;

    fn play(game: &mut BattleGame, from: Square, to: Square) {
        game.play(game.legal_moves_between(from, to)[0]).unwrap();
    }

    fn capture_app(en_passant: bool, animations: bool) -> App {
        let mut game = BattleGame::new();
        play(&mut game, Square::E2, Square::E4);
        let from = if en_passant {
            play(&mut game, Square::A7, Square::A6);
            play(&mut game, Square::E4, Square::E5);
            Square::E5
        } else {
            Square::E4
        };
        play(&mut game, Square::D7, Square::D5);
        let to = if en_passant { Square::D6 } else { Square::D5 };
        let mv = game.legal_moves_between(from, to)[0];
        let mut scene_sync = SceneSync::default();
        let mut fx_pause = FxPause::default();
        commit_move(
            &mut game,
            mv,
            &mut InteractionState::default(),
            &mut CpuSettings::default(),
            &ChessClock::default(),
            &BattleSettings { animations },
            &mut scene_sync,
            &mut fx_pause,
        );

        let mut app = App::new();
        app.insert_resource(game)
            .insert_resource(scene_sync)
            .insert_resource(fx_pause)
            .insert_resource(SceneHandles {
                cube: default(),
                sphere: default(),
                bishop_mitre: default(),
                queen_circlet: default(),
                light_square: default(),
                dark_square: default(),
                board_base: default(),
                white_piece: default(),
                black_piece: default(),
                selected: default(),
                legal: default(),
                capture: default(),
                cursor: default(),
                fx: default(),
            })
            .init_resource::<Time>()
            .init_resource::<InteractionState>()
            .init_resource::<Appearance>()
            .add_systems(Update, (capture_fx_system, sync_scene_system).chain());
        app
    }

    fn advance(app: &mut App, seconds: f32) {
        app.world_mut()
            .resource_mut::<Time>()
            .advance_by(Duration::from_secs_f32(seconds));
        app.update();
    }

    #[test]
    fn each_piece_spawns_its_own_colored_volley() {
        let mut colors = Vec::new();
        for (role, count) in ATTACK_ROLES.into_iter().zip([1, 2, 3, 4, 8, 12]) {
            let mut app = capture_app(false, true);
            app.world_mut()
                .resource_mut::<SceneSync>()
                .pending_fx
                .as_mut()
                .unwrap()
                .role = role;
            let mut materials = Assets::<StandardMaterial>::default();
            let palette = create_attack_materials(&mut materials);
            app.world_mut().resource_mut::<SceneHandles>().fx = palette;
            advance(&mut app, 0.0);
            let world = app.world_mut();
            let mut query =
                world.query::<(&Projectile, &MeshMaterial3d<StandardMaterial>, &Visibility)>();
            let shots: Vec<_> = query.iter(world).collect();
            assert_eq!(shots.len(), count);
            assert_eq!(
                shots
                    .iter()
                    .filter(|(_, _, visibility)| **visibility == Visibility::Visible)
                    .count(),
                1
            );
            let color = materials.get(&shots[0].1.0).unwrap().base_color;
            assert!(
                !colors.contains(&color),
                "each piece needs a distinct color"
            );
            colors.push(color);
            for (projectile, material, _) in shots {
                assert_eq!(projectile.role, role);
                assert_eq!(materials.get(&material.0).unwrap().base_color, color);
                assert!(projectile.position(0.0).distance(projectile.start) < 0.0001);
                assert!(projectile.position(1.0).distance(projectile.end) < 0.0001);
                assert!(projectile.position(0.5).is_finite());
            }
            // Every volley must finish its last explosion before applying the move.
            for _ in 0..50 {
                advance(&mut app, 0.05);
                let world = app.world_mut();
                let shots_left = world.query::<&Projectile>().iter(world).count();
                let debris_left = world.query::<&Debris>().iter(world).count();
                let game = world.resource::<BattleGame>();
                if shots_left + debris_left > 0 {
                    assert_eq!(game.history_len(), 2);
                    assert_eq!(game.piece_at(Square::D5), Some(ChessColor::Black.pawn()));
                }
            }
            assert_eq!(app.world().resource::<BattleGame>().history_len(), 3);
            assert!(app.world().resource::<SceneSync>().pending_move.is_none());
        }
    }

    #[test]
    fn delayed_projectile_keeps_capture_pending_between_explosions() {
        let mut app = capture_app(false, true);
        app.world_mut()
            .resource_mut::<SceneSync>()
            .pending_fx
            .as_mut()
            .unwrap()
            .role = Role::Queen;
        advance(&mut app, 0.0);
        let world = app.world_mut();
        for mut projectile in world.query::<&mut Projectile>().iter_mut(world) {
            if projectile.index == 7 {
                projectile.elapsed = -2.0;
            }
        }
        for _ in 0..19 {
            advance(&mut app, 0.1);
        }
        assert_eq!(
            app.world_mut().query::<&Debris>().iter(app.world()).count(),
            0
        );
        assert_eq!(
            app.world_mut()
                .query::<&Projectile>()
                .iter(app.world())
                .count(),
            1
        );
        assert_eq!(app.world().resource::<BattleGame>().history_len(), 2);
        advance(&mut app, 1.0); // Last shot impacts, but its explosion is still active.
        assert_eq!(app.world().resource::<BattleGame>().history_len(), 2);
        advance(&mut app, 0.65);
        assert_eq!(app.world().resource::<BattleGame>().history_len(), 3);
    }

    #[test]
    fn capture_waits_for_entire_explosion_including_en_passant() {
        for en_passant in [false, true] {
            let mut app = capture_app(en_passant, true);
            let history_len = app.world().resource::<BattleGame>().history_len();
            let from = if en_passant { Square::E5 } else { Square::E4 };
            let to = if en_passant { Square::D6 } else { Square::D5 };
            advance(&mut app, 0.0); // Spawn the projectile at the original position.
            advance(&mut app, 0.17);
            let game = app.world().resource::<BattleGame>();
            assert_eq!(game.piece_at(from), Some(ChessColor::White.pawn()));
            assert_eq!(game.piece_at(Square::D5), Some(ChessColor::Black.pawn()));
            assert_eq!(game.side_to_move(), ChessColor::White);
            assert_eq!(game.history_len(), history_len);
            assert!(game.captured_pieces().is_empty());

            advance(&mut app, 0.18);
            assert!(app.world().resource::<SceneSync>().exploding);
            assert_eq!(
                app.world_mut().query::<&Debris>().iter(app.world()).count(),
                12
            );
            // Even after most fragments expire, neither piece moves early.
            advance(&mut app, 0.60);
            let game = app.world().resource::<BattleGame>();
            assert_eq!(game.piece_at(from), Some(ChessColor::White.pawn()));
            assert_eq!(game.piece_at(Square::D5), Some(ChessColor::Black.pawn()));
            assert_eq!(game.history_len(), history_len);
            assert_eq!(game.side_to_move(), ChessColor::White);
            assert!(game.captured_pieces().is_empty());

            advance(&mut app, 0.05);
            assert_eq!(
                app.world_mut().query::<&Debris>().iter(app.world()).count(),
                0
            );
            let game = app.world().resource::<BattleGame>();
            assert_eq!(game.piece_at(from), None);
            assert_eq!(game.piece_at(to), Some(ChessColor::White.pawn()));
            if en_passant {
                assert_eq!(game.piece_at(Square::D5), None);
            }
            assert_eq!(game.side_to_move(), ChessColor::Black);
            assert_eq!(game.history_len(), history_len + 1);
            assert_eq!(game.captured_pieces().len(), 1);
            assert!(app.world().resource::<SceneSync>().pending_move.is_none());
            advance(&mut app, 1.0);
            assert_eq!(
                app.world().resource::<BattleGame>().history_len(),
                history_len + 1
            );
        }
    }

    #[test]
    fn disabling_animations_captures_immediately() {
        let mut app = capture_app(false, false);
        let game = app.world().resource::<BattleGame>();
        assert_eq!(game.piece_at(Square::E4), None);
        assert_eq!(game.piece_at(Square::D5), Some(ChessColor::White.pawn()));
        assert_eq!(game.captured_pieces().len(), 1);
        advance(&mut app, 0.0);
        assert_eq!(
            app.world_mut()
                .query::<&Projectile>()
                .iter(app.world())
                .count(),
            0
        );
    }

    #[test]
    fn pending_capture_blocks_additional_moves() {
        let mut app = capture_app(false, true);
        let world = app.world_mut();
        let mut game = world.remove_resource::<BattleGame>().unwrap();
        let mut scene_sync = world.remove_resource::<SceneSync>().unwrap();
        let pending = scene_sync.pending_move;
        let mv = game.legal_moves_between(Square::G1, Square::F3)[0];
        commit_move(
            &mut game,
            mv,
            &mut InteractionState::default(),
            &mut CpuSettings::default(),
            &ChessClock::default(),
            &BattleSettings::default(),
            &mut scene_sync,
            &mut world.resource_mut::<FxPause>(),
        );
        assert_eq!(scene_sync.pending_move, pending);
        assert_eq!(game.history_len(), 2);
        assert_eq!(game.piece_at(Square::G1), Some(ChessColor::White.knight()));
    }

    #[test]
    fn undo_is_single_use_and_reenabled_by_a_new_move() {
        for enabled in [false, true] {
            let mut game = BattleGame::new();
            let mut interaction = InteractionState::default();
            let mut clock = ChessClock::default();
            let mut cpu = CpuSettings {
                enabled,
                ..default()
            };
            let mut scene_sync = SceneSync::default();
            let mut pause = FxPause::default();
            assert!(!scene_sync.can_undo(&game));
            play(&mut game, Square::E2, Square::E4);
            play(&mut game, Square::D7, Square::D5);
            assert!(scene_sync.can_undo(&game));
            for _ in 0..2 {
                perform_undo(
                    &mut game,
                    &mut interaction,
                    &mut clock,
                    &mut cpu,
                    &mut scene_sync,
                    &mut pause,
                );
                assert_eq!(game.history_len(), 1);
                assert_eq!(game.piece_at(Square::E4), Some(ChessColor::White.pawn()));
                assert_eq!(game.piece_at(Square::D7), Some(ChessColor::Black.pawn()));
                assert!(!scene_sync.can_undo(&game));
            }
            let mv = game.legal_moves_between(Square::D7, Square::D5)[0];
            commit_move(
                &mut game,
                mv,
                &mut interaction,
                &mut cpu,
                &clock,
                &BattleSettings::default(),
                &mut scene_sync,
                &mut pause,
            );
            assert!(scene_sync.can_undo(&game));
            perform_undo(
                &mut game,
                &mut interaction,
                &mut clock,
                &mut cpu,
                &mut scene_sync,
                &mut pause,
            );
            assert_eq!(game.history_len(), 1);
            assert!(!scene_sync.can_undo(&game));
        }
    }

    #[test]
    fn undo_during_cpu_capture_only_cancels_that_move() {
        let mut app = capture_app(false, true);
        let world = app.world_mut();
        let mut game = world.remove_resource::<BattleGame>().unwrap();
        let mut scene_sync = world.remove_resource::<SceneSync>().unwrap();
        let mut cpu = CpuSettings {
            enabled: true,
            color: ChessColor::White,
            ..default()
        };
        perform_undo(
            &mut game,
            &mut InteractionState::default(),
            &mut ChessClock::default(),
            &mut cpu,
            &mut scene_sync,
            &mut world.resource_mut::<FxPause>(),
        );
        assert_eq!(game.history_len(), 2);
        assert_eq!(game.side_to_move(), ChessColor::White);
        assert_eq!(game.piece_at(Square::D5), Some(ChessColor::Black.pawn()));
        assert!(scene_sync.pending_move.is_none());
        assert!(!scene_sync.can_undo(&game));
    }

    #[test]
    fn undo_and_new_game_cancel_in_flight_capture() {
        for (reset, elapsed) in [(false, 0.17), (true, 0.17), (false, 0.40), (true, 0.40)] {
            let mut app = capture_app(false, true);
            advance(&mut app, 0.0);
            advance(&mut app, elapsed);
            let world = app.world_mut();
            let mut game = world.remove_resource::<BattleGame>().unwrap();
            let mut scene_sync = world.remove_resource::<SceneSync>().unwrap();
            let mut pause = world.resource_mut::<FxPause>();
            if reset {
                game.reset();
                scene_sync.cancel_capture(&mut pause);
            } else {
                perform_undo(
                    &mut game,
                    &mut InteractionState::default(),
                    &mut ChessClock::default(),
                    &mut CpuSettings::default(),
                    &mut scene_sync,
                    &mut pause,
                );
            }
            assert_eq!(pause.remaining, 0.0);
            world.insert_resource(game);
            world.insert_resource(scene_sync);
            advance(&mut app, 1.0);
            advance(&mut app, 1.0);
            let game = app.world().resource::<BattleGame>();
            assert_eq!(game.history_len(), if reset { 0 } else { 2 });
            assert!(game.captured_pieces().is_empty());
            assert!(app.world().resource::<SceneSync>().pending_move.is_none());
            assert_eq!(
                app.world_mut()
                    .query::<&Projectile>()
                    .iter(app.world())
                    .count(),
                0
            );
            assert_eq!(
                app.world_mut().query::<&Debris>().iter(app.world()).count(),
                0
            );
        }
    }
}
