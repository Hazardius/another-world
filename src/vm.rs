use log::{debug, warn};
use rand::random;

use crate::buffer::Buffer;
use crate::opcode::Opcode;
use crate::parts;
use crate::resource::Resource;
use crate::video::{Palette, Point, Video};
use crate::sys::SDLSys;

const NUM_VARIABLES: usize = 256;
const NUM_THREADS: usize = 64;
const SET_INACTIVE_THREAD: usize = 0xfffe;
const INACTIVE_THREAD: usize = 0xffff;
const COLOR_BLACK: u8 = 0xff;
const DEFAULT_ZOOM: u16 = 0x40;
const STACK_SIZE: usize = 0xff;

const VM_VARIABLE_RANDOM_SEED: usize = 0x3c;
const VM_VARIABLE_SCROLL_Y: usize = 0xf9;
const VM_VARIABLE_PAUSE_SLICES: usize = 0xff;


#[derive(Copy, Clone)]
struct Thread {
    pc: usize,
    requested_pc_offset: Option<usize>,
    is_channel_active_current: bool,
    is_channel_active_requested: bool,
}

impl Thread {
    fn new() -> Thread {
        Thread {
            pc: INACTIVE_THREAD,
            requested_pc_offset: None,
            is_channel_active_current: false,
            is_channel_active_requested: false,
        }
    }
}

pub enum VideoBufferSeg {
    Cinematic,
    Video2,
}

pub struct VirtualMachine {
    variables: [i16; NUM_VARIABLES],
    threads: [Thread; NUM_THREADS],
    resource: Resource,
    video: Video,
    requested_next_part: Option<u16>,
    script_ptr: usize,
    stack_ptr: usize,
    goto_next_thread: bool,
    video_buffer_seg: VideoBufferSeg,
    script_stack_calls: [usize; STACK_SIZE],
    sys: SDLSys,
    last_timestamp: u64,
}

impl VirtualMachine {
    pub fn new(resource: Resource, video: Video, sys: SDLSys) -> VirtualMachine {
        let mut variables = [0; NUM_VARIABLES];
        variables[0x54] = 0x81;
        variables[VM_VARIABLE_RANDOM_SEED] = random::<i16>();
        if cfg!(feature = "bypass_protection") {
            variables[0xb6] = 0x10;
            variables[0xc6] = 0x80;
            variables[0xf2] = 4000;
            variables[0xdc] = 33;
        }
        VirtualMachine {
            variables,
            threads: [Thread::new(); NUM_THREADS],
            resource,
            video,
            requested_next_part: None,
            script_ptr: 0,
            stack_ptr: 0,
            goto_next_thread: false,
            video_buffer_seg: VideoBufferSeg::Cinematic,
            script_stack_calls: [0; STACK_SIZE],
            sys,
            last_timestamp: 0,
        }
    }

    pub fn init_for_part(&mut self, part_id: u16) {
        debug!("init_for_part: {}", part_id);
        // player.stop();
        // mixer.stop_all();

        self.variables[0xe4] = 0x14;

        self.resource.setup_part(part_id);

        for thread in self.threads.iter_mut() {
            thread.pc = 0xffff;
            thread.requested_pc_offset = None;
            thread.is_channel_active_current = false;
            thread.is_channel_active_requested = false;
        }

        self.threads[0].pc = 0;
    }

    pub fn check_thread_requests(&mut self) {
        // Check if a part switch has been requested
        if let Some(part) = self.requested_next_part {
            self.init_for_part(part);
            self.requested_next_part = None;
        }

        // Check if a PAUSE or JUMP has been requested
        for thread_id in 0..NUM_THREADS {
            let requested = self.threads[thread_id].is_channel_active_requested;
            self.threads[thread_id].is_channel_active_current = requested;

            if let Some(pc_offset) = self.threads[thread_id].requested_pc_offset {
                self.threads[thread_id].pc = if pc_offset == SET_INACTIVE_THREAD {
                    INACTIVE_THREAD
                } else {
                    pc_offset
                };
                self.threads[thread_id].requested_pc_offset = None;
            }
        }
    }

    pub fn host_frame(&mut self) {
        for thread_id in 0..self.threads.len() {
            if self.threads[thread_id].is_channel_active_current {
                continue;
            }

            let n = self.threads[thread_id].pc;
            if n != INACTIVE_THREAD {
                //debug!("Start of bytecode: {}", self.resource.seg_bytecode);
                self.script_ptr = self.resource.seg_bytecode + n;
                self.stack_ptr = 0;
                self.goto_next_thread = false;

                debug!("host_frame() thread_id=0x{:02x} n=0x{:02x}", thread_id, n);

                self.execute_thread();

                // Save pc since it will be modified on the next iteration
                self.threads[thread_id].pc = self.script_ptr - self.resource.seg_bytecode;

                debug!("host_frame() thread_id=0x{:02x} pos=0x{:x}", thread_id, self.threads[thread_id].pc);

                // if input.quit { break }....
            }

        }
    }

    fn fetch_byte(&mut self) -> u8 {
        let result = self.resource.read_byte(self.script_ptr);
        self.script_ptr += 1;
        result
    }

    fn fetch_word(&mut self) -> u16 {
        let result = self.resource.read_word(self.script_ptr);
        self.script_ptr += 2;
        result
    }

    fn execute_thread(&mut self) {
        while !self.goto_next_thread {
            //debug!("pc: 0x{:x} Decoding opcode", self.script_ptr);
            let opcode = Opcode::decode(self.fetch_byte());

            match opcode {
                Opcode::MovConst => self.op_mov_const(),
                Opcode::Mov => self.op_mov(),
                Opcode::AddConst => self.op_add_const(),
                Opcode::Call => self.op_call(),
                Opcode::Ret => self.op_ret(),
                Opcode::PauseThread => self.op_pause_thread(),
                Opcode::Jmp => self.op_jmp(),
                Opcode::SetSetVect => self.op_set_set_vect(),
                Opcode::Jnz => self.op_jnz(),
                Opcode::CondJmp => self.op_cond_jmp(),
                Opcode::SetPalette => self.op_set_palette(),
                Opcode::SelectVideoPage => self.op_select_video_page(),
                Opcode::FillVideoPage => self.op_fill_video_page(),
                Opcode::CopyVideoPage => self.op_copy_video_page(),
                Opcode::BlitFrameBuffer => self.op_blit_frame_buffer(),
                Opcode::KillThread => self.op_kill_thread(),
                Opcode::DrawString => self.op_draw_string(),
                Opcode::Or => self.op_or(),
                Opcode::PlaySound => self.op_play_sound(),
                Opcode::UpdateMemList => self.op_update_memlist(),
                Opcode::PlayMusic => self.op_play_music(),
                Opcode::DrawPolySprite(val) => self.op_draw_poly_sprite(val),
                Opcode::DrawPolyBackground(val) => self.op_draw_poly_background(val),
                val => unimplemented!("pc 0x{:x} Unimplemented opcode: {:?}", self.script_ptr, val),
            }
        }
    }

    // Opcode implementation

    fn op_mov_const(&mut self) {
        let variable_id = self.fetch_byte() as usize;
        let value = self.fetch_word() as i16;
        debug!("mov_const(0x{:02x}, {})", variable_id, value);
        self.variables[variable_id] = value;
    }

    fn op_mov(&mut self) {
        let dst_variable_id = self.fetch_byte() as usize;
        let src_variable_id = self.fetch_byte() as usize;
        debug!("mov(0x{:02x}, 0x{:02x})", dst_variable_id, src_variable_id);
        self.variables[dst_variable_id] = self.variables[src_variable_id];
    }

    fn op_add_const(&mut self) {
        // Insert gun sound hack here at some point
        let variable_id = self.fetch_byte() as usize;
        let value = self.fetch_word();
        debug!("add_const(0x{:02x}, {})", variable_id, value);
        self.variables[variable_id] = self.variables[variable_id].wrapping_add(value as i16);

    }

    fn op_call(&mut self) {
        let offset = self.fetch_word();

        debug!("call(0x{:x})", offset);
        self.script_stack_calls[self.stack_ptr] = self.script_ptr - self.resource.seg_bytecode;
        if self.stack_ptr == STACK_SIZE {
            panic!("Stack overflow");
        }
        self.stack_ptr += 1;
        self.script_ptr = self.resource.seg_bytecode + offset as usize;
    }

    fn op_ret(&mut self) {
        debug!("ret()");
        if self.stack_ptr == 0 {
            panic!("Stack underflow!");
        }
        self.stack_ptr -= 1;
        self.script_ptr = self.resource.seg_bytecode + self.script_stack_calls[self.stack_ptr]
    }

    fn op_pause_thread(&mut self) {
        debug!("pause_thread()");
        self.goto_next_thread = true;
    }

    fn op_jmp(&mut self) {
        let pc_offset = self.fetch_word() as usize;
        debug!("op_jmp(0x{:02x})", pc_offset);
        self.script_ptr = self.resource.seg_bytecode + pc_offset;
    }

    fn op_set_set_vect(&mut self) {
        let thread_id = self.fetch_byte() as usize;
        let pc_offset_requested = self.fetch_word() as usize;
        debug!("set_set_vect(0x{:02x}, 0x{:x})", thread_id, pc_offset_requested);
        self.threads[thread_id].requested_pc_offset = Some(pc_offset_requested);
    }

    fn op_jnz(&mut self) {
        let i = self.fetch_byte() as usize;
        debug!("jnz(0x{:02x})", i);
        self.variables[i] = self.variables[i].wrapping_sub(1);
        if self.variables[i] != 0 {
            self.op_jmp();
        } else {
            let _ = self.fetch_word();
        }
    }

    fn op_cond_jmp(&mut self) {
        let opcode = self.fetch_byte();
        let var = self.fetch_byte() as usize;
        let b = self.variables[var];

        let a = if opcode & 0x80 > 0 {
            let var = self.fetch_byte() as usize;
            self.variables[var]
        } else if opcode & 0x40 > 0 {
            self.fetch_word() as i16
        } else {
            self.fetch_byte() as i16
        };
        debug!("op_cond_jmp({}, 0x{:02x}, 0x{:02x})", opcode, b, a);

        let expr = match opcode & 7 {
            0 => b == a,
            1 => b != a,
            2 => b > a,
            3 => b >= a,
            4 => b < a,
            5 => b <= a,
            _ => {
                warn!("op_cond_jmp() invalid condition {}", opcode & 7);
                false
            }
        };
        if expr {
            self.op_jmp();
        } else {
            self.fetch_word();
        }
    }

    fn op_set_palette(&mut self) {
        let palette_id = self.fetch_word();
        debug!("set_palette({})", palette_id);
        let palette_id = (palette_id >> 8) as u8;
        if palette_id >= 32 {
            return;
        }
        let palette_offset = palette_id as usize * 32;
        let start = self.resource.seg_palettes + palette_offset;
        let end = start + 32;
        let palette_data = &self.resource.memory[start..end];
        let palette = Palette::from_bytes(palette_data);
        self.video.palette_requested = Some(palette);
    }

    fn op_select_video_page(&mut self) {
        let frame_buffer_id = self.fetch_byte();
        debug!("select_video_page({})", frame_buffer_id);
        self.video.change_page_ptr1(frame_buffer_id);
    }

    fn op_fill_video_page(&mut self) {
        let page_id = self.fetch_byte();
        let color = self.fetch_byte();
        debug!("fill_video_page({}, {})", page_id, color);
        self.video.fill_video_page(page_id, color);
    }

    fn op_copy_video_page(&mut self) {
        let src_page_id = self.fetch_byte();
        let dst_page_id = self.fetch_byte();
        debug!("copy_video_page({}, {})", src_page_id, dst_page_id);
        self.video.copy_page(src_page_id, dst_page_id, self.variables[VM_VARIABLE_SCROLL_Y]);
    }

    fn op_blit_frame_buffer(&mut self) {
        let page_id = self.fetch_byte();
        debug!("blit_frame_buffer({})", page_id);
        //inp_handle_special_keys();

        let delay = self.sys.get_timestamp() - self.last_timestamp;
        let time_to_sleep = self.variables[VM_VARIABLE_PAUSE_SLICES] as u64 * 20 - delay;

        if time_to_sleep > 0 {
            self.sys.sleep(time_to_sleep);
        }

        self.last_timestamp = self.sys.get_timestamp();

        self.variables[0xf7] = 0;

        self.video.update_display(&mut self.sys, page_id);
    }

    fn op_kill_thread(&mut self) {
        debug!("kill_thread()");
        self.script_ptr = self.resource.seg_bytecode + 0xffff;
        self.goto_next_thread = true;
    }

    fn op_draw_string(&mut self) {
        let string_id = self.fetch_word();
        let x = self.fetch_byte() as u16;
        let y = self.fetch_byte() as u16;
        let color = self.fetch_byte() as u16;
        self.video.draw_string(color, x, y, string_id);
    }

    fn op_or(&mut self) {
        let variable_id = self.fetch_byte() as usize;
        let value = self.fetch_word();
        debug!("or(0x{:02x}, {}", variable_id, value);
        self.variables[variable_id] = (self.variables[variable_id] as u16 | value) as i16;
    }

    fn op_play_sound(&mut self) {
        let _resource_id = self.fetch_word();
        let _freq = self.fetch_byte();
        let _vol = self.fetch_byte();
        let _channel = self.fetch_byte();
        warn!("Not implemented");
    }

    fn op_update_memlist(&mut self) {
        let resource_id = self.fetch_word();
        debug!("update_memlist({})", resource_id);

        if resource_id == 0 {
            // self.player.stop();
            // self.mixer.stop_all();
            self.resource.invalidate_resource();
        } else {
            if resource_id >= parts::GAME_PART_FIRST {
                self.requested_next_part = Some(resource_id);
            } else {
                self.resource.load_memory_entry(resource_id);
            }
        }
    }

    fn op_play_music(&mut self) {
        let _resource_id = self.fetch_word();
        let _delay = self.fetch_word();
        let _pos = self.fetch_byte();
        warn!("play_music() not implemented");
    }

    fn op_draw_poly_sprite(&mut self, val: u8) {
        let offset = (self.fetch_word() * 2) as usize;
        let mut x = self.fetch_byte() as i16;
        self.video_buffer_seg = VideoBufferSeg::Cinematic;

        if val & 0x20 == 0 { // bit 0010 0000
            if val & 0x10 == 0 { // bit 0001 000
                x = (x << 8) | self.fetch_byte() as i16;
            } else {
                x = self.variables[x as usize];
            }
        } else {
            if val & 0x10 > 0 { // bit 0001 0000
                x += 0x100;
            }
        }

        let mut y = self.fetch_byte() as i16;
        if val & 8 == 0 { // bit 0000 1000
            if val & 4 == 0 { // bit 0000 0100
                y = (y << 8) | self.fetch_byte() as i16;
            } else {
                y = self.variables[y as usize];
            }
        }

        let mut zoom = self.fetch_byte() as u16;

        if val & 2 == 0 { // bit 0000 0010
            if val & 1 == 0 { // bit 0000 0001
                self.script_ptr -= 1;
                zoom = 0x40;
            } else {
                zoom = self.variables[zoom as usize] as u16;
            }
        } else {
            if val & 1 > 0 { // bit 0000 0001
                self.video_buffer_seg = VideoBufferSeg::Video2;
                self.script_ptr -= 1;
                zoom = 0x40;
            }
        }
        debug!("draw_poly_sprite() offset=0x{:x}, x={}, y={}, zoom={}", offset, x, y, zoom);
        let segment = match self.video_buffer_seg {
            VideoBufferSeg::Cinematic => self.resource.seg_cinematic,
            VideoBufferSeg::Video2 => self.resource.seg_video2,
        };
        let mut buffer = Buffer::with_offset(
            &self.resource.memory[segment..],
            offset
        );
        let color = 0xff;
        let point = Point { x, y };
        self.video.read_and_draw_polygon(&mut buffer, color, zoom, point);
    }

    fn op_draw_poly_background(&mut self, val: u8) {
        let lsb = self.fetch_byte() as u16;

        // Avoid overflow when calculating offset by removing the
        // most significant bit
        let msb = (val & 0x7f) as u16;
        let offset: usize = (((msb << 8) | lsb) * 2) as usize;
        self.video_buffer_seg = VideoBufferSeg::Cinematic;

        let mut x = self.fetch_byte() as i16;
        let mut y = self.fetch_byte() as i16;
        let h = y - 199;
        if h > 0 {
            y = 199;
            x += h;
        }
        debug!(
            "DrawPolyBackground: val: 0x{:02x} off={} x={} y={}",
            val, offset, x, y
        );

        let mut buffer = Buffer::with_offset(
            &self.resource.memory[self.resource.seg_cinematic..],
            offset
        );
        let point = Point { x, y };
        self.video.read_and_draw_polygon(&mut buffer, COLOR_BLACK, DEFAULT_ZOOM, point);
    }
}
