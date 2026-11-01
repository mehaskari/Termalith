//! Telnet as a pure codec: incoming bytes are split into terminal data and
//! the replies the negotiation requires; outgoing keystrokes are escaped.
//!
//! Supported options: the server may ECHO and SUPPRESS-GO-AHEAD; we offer
//! NAWS (window size) and TERMINAL-TYPE. Everything else is refused.
//! Telnet is unencrypted; the UI says so.

const IAC: u8 = 255;
const DONT: u8 = 254;
const DO: u8 = 253;
const WONT: u8 = 252;
const WILL: u8 = 251;
const SB: u8 = 250;
const SE: u8 = 240;

const ECHO: u8 = 1;
const SGA: u8 = 3;
const TTYPE: u8 = 24;
const NAWS: u8 = 31;
const TTYPE_IS: u8 = 0;
const TTYPE_SEND: u8 = 1;
/// Longest subnegotiation accepted; longer ones are dropped.
const MAX_SB: usize = 512;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Data,
    Iac,
    Verb(u8),
    Sb,
    SbIac,
}

/// What came out of a chunk of server bytes.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Decoded {
    /// Bytes for the terminal.
    pub data: Vec<u8>,
    /// Bytes to send back to the server.
    pub reply: Vec<u8>,
}

/// Negotiation state for one connection.
pub struct Telnet {
    state: State,
    sb: Vec<u8>,
    naws: bool,
    size: (u16, u16),
    term: &'static str,
    /// Options we already answered, to avoid negotiation loops.
    answered: [Option<u8>; 256],
}

impl Telnet {
    pub fn new(cols: u16, rows: u16) -> Self {
        Self {
            state: State::Data,
            sb: Vec::new(),
            naws: false,
            size: (cols, rows),
            term: "XTERM-256COLOR",
            answered: [None; 256],
        }
    }

    /// Splits server bytes into terminal data and negotiation replies.
    pub fn decode(&mut self, input: &[u8]) -> Decoded {
        let mut out = Decoded::default();
        for &b in input {
            self.state = match (self.state, b) {
                (State::Data, IAC) => State::Iac,
                (State::Data, b) => {
                    out.data.push(b);
                    State::Data
                }
                (State::Iac, IAC) => {
                    out.data.push(IAC);
                    State::Data
                }
                (State::Iac, v @ (DO | DONT | WILL | WONT)) => State::Verb(v),
                (State::Iac, SB) => {
                    self.sb.clear();
                    State::Sb
                }
                (State::Iac, _) => State::Data, // NOP, GA, etc.
                (State::Verb(v), opt) => {
                    self.negotiate(v, opt, &mut out.reply);
                    State::Data
                }
                (State::Sb, IAC) => State::SbIac,
                (State::Sb, b) => {
                    if self.sb.len() < MAX_SB {
                        self.sb.push(b);
                    }
                    State::Sb
                }
                (State::SbIac, SE) => {
                    self.subnegotiation(&mut out.reply);
                    State::Data
                }
                (State::SbIac, IAC) => {
                    if self.sb.len() < MAX_SB {
                        self.sb.push(IAC);
                    }
                    State::Sb
                }
                (State::SbIac, _) => State::Data,
            };
        }
        out
    }

    fn answer(&mut self, verb: u8, opt: u8, reply: &mut Vec<u8>) {
        // Only reply when our answer changes, per RFC 1143's loop rule.
        if self.answered[usize::from(opt)] == Some(verb) {
            return;
        }
        self.answered[usize::from(opt)] = Some(verb);
        reply.extend_from_slice(&[IAC, verb, opt]);
    }

    fn negotiate(&mut self, verb: u8, opt: u8, reply: &mut Vec<u8>) {
        match (verb, opt) {
            (WILL, ECHO | SGA) => self.answer(DO, opt, reply),
            (WILL, _) => self.answer(DONT, opt, reply),
            (DO, NAWS) => {
                self.answer(WILL, NAWS, reply);
                self.naws = true;
                reply.extend_from_slice(&self.naws_bytes());
            }
            (DO, TTYPE) => self.answer(WILL, TTYPE, reply),
            (DO, _) => self.answer(WONT, opt, reply),
            (DONT, NAWS) => {
                self.naws = false;
                self.answer(WONT, NAWS, reply);
            }
            (DONT, _) => self.answer(WONT, opt, reply),
            (WONT, _) => self.answer(DONT, opt, reply),
            _ => {}
        }
    }

    fn subnegotiation(&mut self, reply: &mut Vec<u8>) {
        if self.sb.as_slice() == [TTYPE, TTYPE_SEND] {
            reply.extend_from_slice(&[IAC, SB, TTYPE, TTYPE_IS]);
            reply.extend_from_slice(self.term.as_bytes());
            reply.extend_from_slice(&[IAC, SE]);
        }
    }

    fn naws_bytes(&self) -> Vec<u8> {
        let mut v = vec![IAC, SB, NAWS];
        for b in self
            .size
            .0
            .to_be_bytes()
            .into_iter()
            .chain(self.size.1.to_be_bytes())
        {
            v.push(b);
            if b == IAC {
                v.push(IAC);
            }
        }
        v.extend_from_slice(&[IAC, SE]);
        v
    }

    /// New window size; returns bytes to send if the server asked for NAWS.
    pub fn resize(&mut self, cols: u16, rows: u16) -> Option<Vec<u8>> {
        self.size = (cols, rows);
        self.naws.then(|| self.naws_bytes())
    }

    /// Escapes keystrokes for the wire: 0xFF doubled, Enter (CR) sent as
    /// CR LF (Telnet's end of line).
    pub fn encode(input: &[u8]) -> Vec<u8> {
        let mut out = Vec::with_capacity(input.len() + 4);
        for &b in input {
            match b {
                IAC => out.extend_from_slice(&[IAC, IAC]),
                b'\r' => out.extend_from_slice(b"\r\n"),
                b => out.push(b),
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_data_and_escaped_iac() {
        let mut t = Telnet::new(80, 24);
        let d = t.decode(b"hello \xff\xff world");
        assert_eq!(d.data, b"hello \xff world");
        assert!(d.reply.is_empty());
    }

    #[test]
    fn negotiation_accepts_echo_sga_naws_ttype_and_refuses_the_rest() {
        let mut t = Telnet::new(132, 43);
        let d = t.decode(&[
            IAC, WILL, ECHO, IAC, WILL, SGA, IAC, DO, NAWS, IAC, DO, TTYPE, IAC, DO, 39, IAC, WILL,
            34,
        ]);
        assert!(d.data.is_empty());
        let mut want = vec![
            IAC, DO, ECHO, IAC, DO, SGA, IAC, WILL, NAWS, IAC, SB, NAWS, 0, 132, 0, 43, IAC, SE,
        ];
        want.extend_from_slice(&[IAC, WILL, TTYPE, IAC, WONT, 39, IAC, DONT, 34]);
        assert_eq!(d.reply, want);
        // Repeated requests get no repeated answers (no loops).
        assert!(t.decode(&[IAC, WILL, ECHO, IAC, DO, 39]).reply.is_empty());
    }

    #[test]
    fn terminal_type_and_resize() {
        let mut t = Telnet::new(80, 24);
        assert_eq!(t.resize(100, 30), None, "no NAWS until the server asks");
        t.decode(&[IAC, DO, NAWS]);
        assert_eq!(
            t.resize(255, 40).unwrap(),
            vec![IAC, SB, NAWS, 0, 255, 255, 0, 40, IAC, SE],
            "a 255 byte is doubled"
        );
        let d = t.decode(&[IAC, SB, TTYPE, TTYPE_SEND, IAC, SE]);
        let mut want = vec![IAC, SB, TTYPE, TTYPE_IS];
        want.extend_from_slice(b"XTERM-256COLOR");
        want.extend_from_slice(&[IAC, SE]);
        assert_eq!(d.reply, want);
    }

    #[test]
    fn sequences_split_across_chunks() {
        let mut t = Telnet::new(80, 24);
        let a = t.decode(&[b'a', IAC]);
        let b = t.decode(&[WILL]);
        let c = t.decode(&[ECHO, b'b']);
        assert_eq!([a.data, b.data, c.data].concat(), b"ab");
        assert_eq!(c.reply, vec![IAC, DO, ECHO]);
    }

    #[test]
    fn encode_escapes_and_sends_crlf() {
        assert_eq!(Telnet::encode(b"ls\r"), b"ls\r\n");
        assert_eq!(Telnet::encode(&[1, IAC, 2]), vec![1, IAC, IAC, 2]);
    }

    #[test]
    fn oversized_subnegotiation_is_dropped() {
        let mut t = Telnet::new(80, 24);
        let mut input = vec![IAC, SB, TTYPE];
        input.extend(std::iter::repeat_n(b'x', 10_000));
        input.extend_from_slice(&[IAC, SE, b'k']);
        let d = t.decode(&input);
        assert_eq!(d.data, b"k");
        assert!(d.reply.is_empty());
    }
}
