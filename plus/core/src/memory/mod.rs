//! Memory and history per repository: auto memory, instruction files and sessions (D-144, D-145).
//! Every operation takes explicit roots and a data directory, so tests run in temp dirs and
//! nothing reads the real `~/.claude`.

pub mod resolve;
