use crate::instruction::{Instruction, instruction_set, extended_instruction_set};
use crate::memory::MemoryChunk;
use log::trace;

pub const INTERRUPTS_ENABLED_ADDRESS: u16 = 0xFFFF;
pub const INTERRUPTS_HAPPENED_ADDRESS: u16 = 0xFF0F;
pub const VBLANK: u8 = 0x1;

/// Gameboy clock state
#[derive(Default, Debug)]
pub struct Clock {
  m: u8,
  t: u8,
}

/// Represents a register pair that can be addressed either as two u8's or one u16
#[derive(Default, Debug)]
pub struct RegisterPair {
  l: u8,
  r: u8
}

impl RegisterPair {
  fn as_u16(&self) -> u16 {
    let high_portion = (self.l as u16) << 8;
    let low_portion = self.r as u16;
    high_portion + low_portion
  }

  fn write_u16(&mut self, v: u16) {
    self.l = (v >> 8) as u8;
    self.r = (v & 0xFF) as u8;
  }
}

/// Enum to address all the 8-bit registers
#[derive(Debug, Clone, Copy)]
pub enum SmallWidthRegister {
  B, C, A, F, D, E, H, L
}

/// Enum to address all the 16-bit wide registers
#[derive(Debug, Clone, Copy)]
pub enum WideRegister {
  PC, SP, BC, AF, DE, HL
}

use SmallWidthRegister::*;
use WideRegister::*;

/// CPU state and registers for a Z80 gameboy processor.
#[derive(Default, Debug)]
pub struct Registers {
  pc: u16,
  sp: u16,
  bc: RegisterPair,
  af: RegisterPair,
  de: RegisterPair,
  hl: RegisterPair,
  clock: Clock,

  /// Interrupts master enable flag
  pub ime: bool,

  /// This indicates the last opcode processed was an escape opcode, triggering the extended instruction set
  pub escaped: bool,

  /// How many cycles passed in the last CPU step
  pub last_clock: u16, 
}

pub const ZERO_FLAG: u8 = 0x1 << 6;
pub const HALF_CARRY_FLAG: u8 = 0x1 << 4;
pub const SUBTRACT_FLAG: u8 = 0x1 << 2;
pub const INTERRUPTS: u16 = 0xFFFF;

/// The position of the CARRY bit in the F (flags) register
pub const CARRY_FLAG: u8 = 0x1;

impl Registers {

  pub fn pc(&self) -> u16 { self.read_r16(WideRegister::PC) } 
  pub fn sp(&self) -> u16 { self.read_r16(WideRegister::SP) }

  pub fn set_pc(&mut self, pc: u16) { self.write_r16(WideRegister::PC, pc); }
  pub fn set_sp(&mut self, sp: u16) { self.write_r16(WideRegister::SP, sp); }

  pub fn inc_pc(&mut self, by: u16) {
    self.write_r16(WideRegister::PC, self.read_r16(WideRegister::PC) + by);
  }

  pub fn dec_pc(&mut self, by: u16) {
    self.write_r16(WideRegister::PC, self.read_r16(WideRegister::PC) - by);
  }

  /// Push a 16 bit value from the stack and return it
  pub fn stack_push16(&mut self, value: u16, memory: &mut Box<dyn MemoryChunk>) {
    let new_sp = self.sp() - 2;
    memory.write_u16(new_sp, value);
    self.set_sp(new_sp);
  }

  /// Pop a 16 bit value from the stack and return it
  pub fn stack_pop16(&mut self, memory: &mut Box<dyn MemoryChunk>) -> u16 {
    let sp = self.sp();
    let rval = memory.read_u16(sp);
    self.set_sp(sp + 2);
    rval
  }

  pub fn jump_relative(&mut self, by: i8) {
    // TODO: I'm not sure if this is correct
    // there must be a better way to do signed and unsigned addition in Rust
    if by > 0 {
      self.inc_pc(by as u16);
    } else {
      self.dec_pc(-by as u16);
    }
  }

  pub fn read_r8(&self, reg: SmallWidthRegister) -> u8 {
    match reg {
      B => self.bc.l,
      C => self.bc.r,
      A => self.af.l,
      F => self.af.r,
      D => self.de.l,
      E => self.de.r,
      H => self.hl.l,
      L => self.hl.r
    }
  }

  pub fn write_r8(&mut self, reg: SmallWidthRegister, val: u8) {
    match reg {
      B => self.bc.l = val,
      C => self.bc.r = val,
      A => self.af.l = val,
      F => self.af.r = val,
      D => self.de.l = val,
      E => self.de.r = val,
      H => self.hl.l = val,
      L => self.hl.r = val
    };
  }

  pub fn read_r16(&self, reg: WideRegister) -> u16 {
    match reg {
      PC => self.pc,
      SP => self.sp,
      BC => self.bc.as_u16(),
      AF => self.af.as_u16(),
      DE => self.de.as_u16(),
      HL => self.hl.as_u16()
    }
  }

  pub fn write_r16(&mut self, reg: WideRegister, val: u16) {
    trace!("Writing {:x} -> {:?}", val, reg);
    match reg {
      PC => {
        self.pc = val
      },
      SP => {
        self.sp = val;
      },
      BC => self.bc.write_u16(val),
      AF => self.af.write_u16(val),
      DE => self.de.write_u16(val),
      HL => self.hl.write_u16(val)
    };
  }

  pub fn flags(&self) -> u8 {
    self.read_r8(SmallWidthRegister::F)
  }

  pub fn zero(&self) -> bool {
    self.flags() & ZERO_FLAG != 0
  }

  pub fn carry(&self) -> bool {
    self.flags() & CARRY_FLAG != 0
  }

  pub fn set_carry(&mut self, state: bool) {
    let mut current_flags = self.read_r8(SmallWidthRegister::F);
    current_flags = Registers::set_flag(current_flags, CARRY_FLAG, state);
    self.write_r8(SmallWidthRegister::F, current_flags);
  }

  pub fn set_flag(flags: u8, flag: u8, set: bool) -> u8 {
    if set {
      flags | flag
    } else {
      flags & !flag
    }
  }

  pub fn set_flags(&mut self,
    zero: bool,
    negative: bool,
    half_carry: bool,
    carry: bool) {
    trace!("Set flags to Z: {} N: {} H: {} C: {}", zero, negative, half_carry, carry);
    let mut current_flags = self.read_r8(SmallWidthRegister::F);
    current_flags = Registers::set_flag(current_flags, CARRY_FLAG, carry);
    current_flags = Registers::set_flag(current_flags, HALF_CARRY_FLAG, half_carry);
    current_flags = Registers::set_flag(current_flags, SUBTRACT_FLAG, negative);
    current_flags = Registers::set_flag(current_flags, ZERO_FLAG, zero);
    self.write_r8(SmallWidthRegister::F, current_flags);
  }
}

pub struct CPU {
  pub registers: Registers,
  instructions: Vec<Instruction>,
  ext_instructions: Vec<Instruction>,
}

impl CPU {
  pub fn new() -> CPU {
    let instructions = instruction_set();
    let ext = extended_instruction_set();
    for i in 0..instructions.len() {
      println!("{:x}:{}", i, instructions[i].text);
    }
    for i in 0..ext.len() {
      println!("{:x}:{}", i, ext[i].text);
    }
    CPU {
      registers: Registers::default(),
      instructions: instructions,
      ext_instructions: ext,
    }
  }

  fn fire_interrupt(&mut self, location: u16, memory: &mut Box<dyn MemoryChunk>) {
    self.registers.ime = false;
    self.registers.stack_push16(self.registers.pc(), memory);
    self.registers.set_pc(location);
  }

  pub fn check_interrupt(&mut self, memory: &mut Box<dyn MemoryChunk>) {
    if self.registers.ime {
      let enabled = memory.read_u8(INTERRUPTS_ENABLED_ADDRESS);
      let triggered = memory.read_u8(INTERRUPTS_HAPPENED_ADDRESS);
      let interrupted = triggered & enabled;
      if interrupted & VBLANK != 0 {
        // VBLANK
        trace!("VBLANK INTERRUPT");
        memory.write_u8(INTERRUPTS_HAPPENED_ADDRESS, triggered & !VBLANK);
        self.fire_interrupt(0x40, memory);
        return;
      }
    }
  }

  pub fn step(&mut self, memory: &mut Box<dyn MemoryChunk>) {
    let opcode = memory.read_u8(self.registers.pc());
    trace!(
      "pre\nPC={:x} SP={:x} BC={:x} AF={:x} DE={:x} HL={:x}\n B={:x} C={:x} A={:x} F={:x} D={:x} E={:x} H={:x} L={:x}",
      self.registers.pc(),
      self.registers.sp(),
      self.registers.read_r16(WideRegister::BC),
      self.registers.read_r16(WideRegister::AF),
      self.registers.read_r16(WideRegister::DE),
      self.registers.read_r16(WideRegister::HL),
      self.registers.bc.l, self.registers.bc.r,
      self.registers.af.l, self.registers.af.r,
      self.registers.de.l, self.registers.de.r,
      self.registers.hl.l, self.registers.hl.r,
    );

    let inst;

    if self.registers.escaped {
      trace!("Selected opcode from extended set since escaped is set");
      inst = &self.ext_instructions[opcode as usize];
      self.registers.escaped = false;
    } else {
      inst = &self.instructions[opcode as usize];
    }

    trace!("{} ({:x})", inst.text, opcode);
    (inst.execute)(&mut self.registers, memory, &inst.data);
    //trace!("post-step: {:?}", self.registers);
    self.registers.last_clock = inst.timings.1;

    // if ime is flagged on and there is an interrupt waiting then trigger it
    self.check_interrupt(memory);
  }
}
