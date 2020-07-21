/* Copyright 2020 Matt Spraggs
 *
 * Licensed under the Apache License, Version 2.0 (the "License");
 * you may not use this file except in compliance with the License.
 * You may obtain a copy of the License at
 *
 *     http://www.apache.org/licenses/LICENSE-2.0
 *
 * Unless required by applicable law or agreed to in writing, software
 * distributed under the License is distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 * See the License for the specific language governing permissions and
 * limitations under the License.
 */

use crate::value;

#[repr(u8)]
pub enum OpCode {
    Constant,
    Nil,
    True,
    False,
    Pop,
    GetLocal,
    SetLocal,
    GetGlobal,
    DefineGlobal,
    SetGlobal,
    Equal,
    Greater,
    Less,
    Add,
    Subtract,
    Multiply,
    Divide,
    Not,
    Negate,
    Print,
    Jump,
    JumpIfFalse,
    Loop,
    Call,
    Return,
}

impl From<u8> for OpCode {
    fn from(value: u8) -> Self {
        match value {
            value if value == OpCode::Constant as u8 => OpCode::Constant,
            value if value == OpCode::Nil as u8 => OpCode::Nil,
            value if value == OpCode::True as u8 => OpCode::True,
            value if value == OpCode::False as u8 => OpCode::False,
            value if value == OpCode::Pop as u8 => OpCode::Pop,
            value if value == OpCode::GetLocal as u8 => OpCode::GetLocal,
            value if value == OpCode::SetLocal as u8 => OpCode::SetLocal,
            value if value == OpCode::GetGlobal as u8 => OpCode::GetGlobal,
            value if value == OpCode::DefineGlobal as u8 => {
                OpCode::DefineGlobal
            }
            value if value == OpCode::SetGlobal as u8 => OpCode::SetGlobal,
            value if value == OpCode::Equal as u8 => OpCode::Equal,
            value if value == OpCode::Greater as u8 => OpCode::Greater,
            value if value == OpCode::Less as u8 => OpCode::Less,
            value if value == OpCode::Add as u8 => OpCode::Add,
            value if value == OpCode::Subtract as u8 => OpCode::Subtract,
            value if value == OpCode::Multiply as u8 => OpCode::Multiply,
            value if value == OpCode::Divide as u8 => OpCode::Divide,
            value if value == OpCode::Not as u8 => OpCode::Not,
            value if value == OpCode::Negate as u8 => OpCode::Negate,
            value if value == OpCode::Print as u8 => OpCode::Print,
            value if value == OpCode::Jump as u8 => OpCode::Jump,
            value if value == OpCode::JumpIfFalse as u8 => OpCode::JumpIfFalse,
            value if value == OpCode::Loop as u8 => OpCode::Loop,
            value if value == OpCode::Call as u8 => OpCode::Call,
            value if value == OpCode::Return as u8 => OpCode::Return,
            _ => panic!("Unknown opcode {}", value),
        }
    }
}

#[derive(Clone, Default)]
pub struct Chunk {
    pub code: Vec<u8>,
    pub lines: Vec<i32>,
    pub constants: Vec<value::Value>,
}

impl Chunk {
    pub fn new() -> Self {
        Default::default()
    }

    pub fn write(&mut self, byte: u8, line: i32) {
        self.code.push(byte);
        self.lines.push(line);
    }

    pub fn add_constant(&mut self, value: value::Value) -> usize {
        self.constants.push(value);
        self.constants.len() - 1
    }
}
