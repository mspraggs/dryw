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
    Equal,
    Greater,
    Less,
    Add,
    Subtract,
    Multiply,
    Divide,
    Not,
    Negate,
    Return,
}

impl From<u8> for OpCode {
    fn from(value: u8) -> Self {
        match value {
            value if value == OpCode::Constant as u8 => OpCode::Constant,
            value if value == OpCode::Nil as u8 => OpCode::Nil,
            value if value == OpCode::True as u8 => OpCode::True,
            value if value == OpCode::False as u8 => OpCode::False,
            value if value == OpCode::Equal as u8 => OpCode::Equal,
            value if value == OpCode::Greater as u8 => OpCode::Greater,
            value if value == OpCode::Less as u8 => OpCode::Less,
            value if value == OpCode::Add as u8 => OpCode::Add,
            value if value == OpCode::Subtract as u8 => OpCode::Subtract,
            value if value == OpCode::Multiply as u8 => OpCode::Multiply,
            value if value == OpCode::Divide as u8 => OpCode::Divide,
            value if value == OpCode::Not as u8 => OpCode::Not,
            value if value == OpCode::Negate as u8 => OpCode::Negate,
            value if value == OpCode::Return as u8 => OpCode::Return,
            _ => panic!("Unknown opcode {}", value),
        }
    }
}

#[derive(Default)]
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
