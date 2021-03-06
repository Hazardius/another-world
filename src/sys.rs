use log::debug;
use std::sync::{Arc, RwLock};
use std::{thread, time};

use sdl2::audio::{AudioDevice, AudioSpecDesired};
use sdl2::event::Event;
use sdl2::keyboard::Keycode;
use sdl2::pixels::{Color, Palette, PixelFormatEnum};
use sdl2::rect::Rect;
use sdl2::render::WindowCanvas;
use sdl2::surface::Surface;
use sdl2::EventPump;

use crate::mixer;
use crate::player::{PlayerDirection, PlayerInput};
use crate::video;

pub struct SDLSys {
    sdl_context: sdl2::Sdl,
    surface: Surface<'static>,
    canvas: WindowCanvas,
    audio_device: Option<AudioDevice<mixer::MixerAudio>>,
    timestamp: time::Instant,
    event_pump: EventPump,
    player_input: PlayerInput,
    width: usize,
    height: usize,
}

impl SDLSys {
    pub fn new(sdl_context: sdl2::Sdl, width: usize, height: usize) -> SDLSys {
        let video_subsystem = sdl_context.video().unwrap();

        let window = video_subsystem
            .window("Another world", 1280, 800)
            .position_centered()
            .resizable()
            .build()
            .unwrap();

        let mut canvas = window.into_canvas().build().expect("Expected canvas");
        canvas
            .set_logical_size(width as u32, height as u32)
            .expect("Expected logical size");
        let event_pump = sdl_context.event_pump().unwrap();
        SDLSys {
            sdl_context,
            surface: Surface::new(width as u32, height as u32, PixelFormatEnum::Index8).unwrap(),
            canvas,
            audio_device: None,
            timestamp: time::Instant::now(),
            event_pump,
            player_input: PlayerInput::new(),
            width,
            height,
        }
    }

    pub fn set_palette(&mut self, palette: &video::Palette) {
        debug!("set_palette()");
        let colors: Vec<Color> = palette
            .entries
            .iter()
            .map(|c| Color::RGBA(c.r, c.g, c.b, c.a))
            .collect();
        let sdl_palette = Palette::with_colors(&colors).unwrap();

        self.surface.set_palette(&sdl_palette).unwrap();
    }

    pub fn update_display(&mut self, page: &video::Page) {
        debug!("update_display()");
        let pitch = self.surface.pitch() as usize;
        let width = self.width;
        let height = self.height;
        self.surface.with_lock_mut(|p| {
            for j in 0..height {
                let p_offset = pitch * j;
                let page_offset = j * width;
                p[p_offset..(width + p_offset)]
                    .clone_from_slice(&page.data[page_offset..(width + page_offset)]);
            }
        });
        let texture_creator = self.canvas.texture_creator();
        let texture = texture_creator
            .create_texture_from_surface(&*self.surface)
            .unwrap();
        self.canvas.clear();
        self.canvas
            .copy(
                &texture,
                None,
                Some(Rect::new(0, 0, width as u32, height as u32)),
            )
            .unwrap();
        self.canvas.present();
    }

    pub fn sleep(&self, ms: u64) {
        let duration = time::Duration::from_millis(ms);
        thread::sleep(duration);
    }

    pub fn get_timestamp(&self) -> u64 {
        (self.timestamp.elapsed().as_millis() & std::u64::MAX as u128) as u64
    }

    pub fn start_audio(&mut self, audio: Arc<RwLock<mixer::Mixer>>) {
        debug!("Starting audio");
        let audio_subsystem = self.sdl_context.audio().unwrap();

        let desired_spec = AudioSpecDesired {
            freq: Some(mixer::SOUND_SAMPLE_RATE as i32),
            channels: Some(1),
            samples: None,
        };

        let device = audio_subsystem
            .open_playback(None, &desired_spec, |spec| {
                debug!("Actual spec: {:?}", spec);
                mixer::MixerAudio(audio)
            })
            .unwrap();

        device.resume();
        self.audio_device = Some(device);
    }

    pub fn process_events(&mut self) -> PlayerInput {
        let mut last_char = '\0';
        for event in self.event_pump.poll_iter() {
            match event {
                Event::Quit { .. }
                | Event::KeyDown {
                    keycode: Some(Keycode::Escape),
                    ..
                } => self.player_input.quit = true,
                Event::KeyDown { keycode, .. } => match keycode.unwrap() {
                    Keycode::Left => self.player_input.direction |= PlayerDirection::LEFT,
                    Keycode::Right => self.player_input.direction |= PlayerDirection::RIGHT,
                    Keycode::Up => self.player_input.direction |= PlayerDirection::UP,
                    Keycode::Down => self.player_input.direction |= PlayerDirection::DOWN,
                    Keycode::LShift | Keycode::Space | Keycode::Return => {
                        self.player_input.button = true
                    }
                    Keycode::Backspace => last_char = '\x08',
                    Keycode::A => {
                        self.player_input.direction |= PlayerDirection::LEFT;
                        last_char = 'A';
                    }
                    Keycode::B => last_char = 'B',
                    Keycode::C => {
                        self.player_input.code = true;
                        last_char = 'C';
                    }
                    Keycode::D => {
                        self.player_input.direction |= PlayerDirection::RIGHT;
                        last_char = 'D';
                    }
                    Keycode::E => last_char = 'E',
                    Keycode::F => last_char = 'F',
                    Keycode::G => last_char = 'G',
                    Keycode::H => last_char = 'H',
                    Keycode::I => last_char = 'I',
                    Keycode::J => last_char = 'J',
                    Keycode::K => last_char = 'K',
                    Keycode::L => last_char = 'L',
                    Keycode::M => last_char = 'M',
                    Keycode::N => last_char = 'N',
                    Keycode::O => last_char = 'O',
                    Keycode::P => last_char = 'P',
                    Keycode::Q => last_char = 'Q',
                    Keycode::R => last_char = 'R',
                    Keycode::S => {
                        self.player_input.direction |= PlayerDirection::DOWN;
                        last_char = 'S';
                    }
                    Keycode::T => last_char = 'T',
                    Keycode::U => last_char = 'U',
                    Keycode::V => last_char = 'V',
                    Keycode::W => {
                        self.player_input.direction |= PlayerDirection::UP;
                        last_char = 'W';
                    }
                    Keycode::X => last_char = 'X',
                    Keycode::Y => last_char = 'Y',
                    Keycode::Z => last_char = 'Z',
                    _ => {}
                },
                Event::KeyUp { keycode, .. } => match keycode.unwrap() {
                    Keycode::Left | Keycode::A => {
                        self.player_input.direction &= !PlayerDirection::LEFT
                    }
                    Keycode::Right | Keycode::D => {
                        self.player_input.direction &= !PlayerDirection::RIGHT
                    }
                    Keycode::Up | Keycode::W => self.player_input.direction &= !PlayerDirection::UP,
                    Keycode::Down | Keycode::S => {
                        self.player_input.direction &= !PlayerDirection::DOWN
                    }
                    Keycode::LShift | Keycode::Space | Keycode::Return => {
                        self.player_input.button = false
                    }
                    _ => {}
                },
                _ => {}
            }
        }
        self.player_input.last_char = last_char;
        let result = self.player_input;
        self.player_input.code = false;
        result
    }
}
