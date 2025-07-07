//! gen_mobjinfo.rs - one-shot CLI to convert Doom’s original `info.c`
//! into Rust source files (`states.rs` + `mobjinfo.rs`).
//!
//! USAGE:
//! ```bash
//! cargo run --bin gen_mobjinfo -- \
//!     --info-c path/to/info.c \
//!     --out-dir ./src/defs
//! ```

use clap::Parser;
use regex::Regex;
use std::collections::BTreeSet;
use std::{fs, path::PathBuf};

/// CLI options handled via `clap` derive.
#[derive(Parser, Debug)]
#[command(author, version, about)]
struct Opts {
    /// Path to `info.c` (vanilla Doom source)
    #[arg(long, value_name = "FILE")]
    info_c: PathBuf,

    /// Directory receiving the generated files
    #[arg(long, value_name = "DIR", default_value = "./src/defs")]
    out_dir: PathBuf,
}

/// Minimal representation of a state_t row.
#[derive(Debug, Clone)]
struct StateRow {
    sprite: String,
    frame: u8,
    tics: i32,
    action: String,
    next_state: String,
    misc1: i32,
    misc2: i32,
    name: String,
}

/// Minimal representation of an mobjinfo_t row.
#[derive(Debug, Clone)]
struct MobjRow {
    id: String,
    doomednum: i32,
    spawnstate: String,
    spawnhealth: i32,
    seestate: String,
    seesound: String,
    reactiontime: i32,
    attacksound: String,
    painstate: String,
    painchance: i32,
    painsound: String,
    meleestate: String,
    missilestate: String,
    deathstate: String,
    xdeathstate: String,
    deathsound: String,
    speed: i32,
    radius: i32,
    height: i32,
    mass: i32,
    damage: i32,
    activesound: String,
    flags: String,
    raisestate: String,
}

// ------------------------------------------------------------------
//  High-level entry point
// ------------------------------------------------------------------
fn main() -> anyhow::Result<()> {
    let opts = Opts::parse();

    // 1. Load the C source.
    let info_c_src = fs::read_to_string(&opts.info_c)?;

    // 2. Grab raw array bodies.
    let states_body =
        extract_array_body("state_t", "states", &info_c_src).expect("states[] not found in info.c");
    let mobj_body = extract_array_body("mobjinfo_t", "mobjinfo", &info_c_src)
        .expect("mobjinfo[] not found in info.c");

    // 3. Parse rows.
    let state_rows: Vec<StateRow> = states_body
        .lines()
        .filter_map(|l| parse_state_line(l))
        .collect();

    let action_names: Vec<String> = {
        let mut set = BTreeSet::new();
        for s in &state_rows {
            if s.action != "NULL" {
                set.insert(s.action.clone());
            }
        }
        set.into_iter().collect()
    };

    let mobj_rows = extract_mobj_rows(&mobj_body);

    let sound_names: Vec<String> = {
        let mut set = BTreeSet::new();
        for m in &mobj_rows {
            set.insert(m.activesound.clone());
            set.insert(m.attacksound.clone());
            set.insert(m.deathsound.clone());
            set.insert(m.painsound.clone());
            set.insert(m.seesound.clone());
        }
        set.into_iter().collect()
    };

    // 4. Emit generated Rust.
    fs::create_dir_all(&opts.out_dir)?;
    fs::write(opts.out_dir.join("state.rs"), render_state(&state_rows))?;
    fs::write(opts.out_dir.join("action.rs"), render_action(&action_names))?;
    fs::write(opts.out_dir.join("sound.rs"), render_sound(&sound_names))?;
    fs::write(opts.out_dir.join("states.rs"), render_states(&state_rows))?;
    fs::write(
        opts.out_dir.join("mobjinfo.rs"),
        render_mobjinfo(&mobj_rows),
    )?;

    println!(
        "Generated {} states and {} mobjs",
        state_rows.len(),
        mobj_rows.len()
    );
    Ok(())
}

// ------------------------------------------------------------------
//  Extractors
// ------------------------------------------------------------------

/// Generic “pull the array body” helper.
fn extract_array_body(ctype: &str, name: &str, src: &str) -> Option<String> {
    let pattern = format!(
        r"(?s){}\s+{}\s*\[[^\]]*\]\s*=\s*\{{(?P<body>.*?)\}};",
        regex::escape(ctype),
        regex::escape(name)
    );
    let re = Regex::new(&pattern).unwrap();
    re.captures(src).map(|c| c["body"].to_string())
}

/// Pull every “{ ... }” struct from the mobjinfo body.
fn extract_mobj_rows(body: &str) -> Vec<MobjRow> {
    let row_re = Regex::new(r"(?s)\{([^}]*)\}").unwrap(); // DOTALL, capture inside braces
    row_re
        .captures_iter(body)
        .filter_map(|cap| parse_mobj_chunk(cap[1].trim())) // cap[1] = inner text
        .collect()
}

// ---------------------------------------------------------------
// Parsers — deliberately tolerant of whitespace & comments.
// ---------------------------------------------------------------

fn parse_state_line(line: &str) -> Option<StateRow> {
    let line = line.trim();
    if !line.starts_with('{') {
        return None;
    }

    // Remove outer braces and trailing comma.
    let content = line.trim_matches(&['{', '}', ','][..]);
    let fields: Vec<&str> = content.split(',').map(|f| f.trim()).collect();
    if fields.len() < 7 {
        return None;
    }

    let action_raw = fields[3]
        .trim_matches(|c| c == '{' || c == '}')
        .trim_start_matches("A_");

    // Look for the trailing “// S_SOMETHING” enum comment.
    let enum_name = line
        .split("//")
        .nth(1)
        .and_then(|c| c.split_whitespace().next())
        .unwrap_or("")
        .trim_start_matches("S_")
        .to_string();

    Some(StateRow {
        sprite: fields[0].trim_start_matches("SPR_").to_string(),
        frame: fields[1].parse().unwrap_or(0),
        tics: fields[2].parse().unwrap_or(0),
        action: action_raw.to_string(),
        next_state: fields[4].trim_start_matches("S_").to_string(),
        misc1: fields[5].parse().unwrap_or(0),
        misc2: fields[6].parse().unwrap_or(0),
        name: enum_name,
    })
}

/// Convert a single mobjinfo initializer (comments already inside) into a row.
fn parse_mobj_chunk(text: &str) -> Option<MobjRow> {
    // 0) capture the leading “// MT_FOO” tag (if any)
    let id = text
        .lines()
        .next()
        .and_then(|l| l.split("//").nth(1).map(|c| c.trim()))
        .unwrap_or("")
        .trim_start_matches("MT_")
        .to_string();

    // strip per-line “// …” comments
    let cleaned = text
        .lines()
        .map(|l| l.split("//").next().unwrap_or("").trim())
        .collect::<Vec<_>>()
        .join(" ");

    let f: Vec<&str> = cleaned
        .split(',')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect();

    if f.len() < 23 {
        return None;
    }

    Some(MobjRow {
        id,
        doomednum: f[0].parse().unwrap_or(-1),
        spawnstate: f[1].trim_start_matches("S_").to_string(),
        spawnhealth: f[2].parse().unwrap_or(0),
        seestate: f[3].trim_start_matches("S_").to_string(),
        seesound: f[4].trim_start_matches("sfx_").to_string(),
        reactiontime: f[5].parse().unwrap_or(0),
        attacksound: f[6].trim_start_matches("sfx_").to_string(),
        painstate: f[7].trim_start_matches("S_").to_string(),
        painchance: f[8].parse().unwrap_or(0),
        painsound: f[9].trim_start_matches("sfx_").to_string(),
        meleestate: f[10].trim_start_matches("S_").to_string(),
        missilestate: f[11].trim_start_matches("S_").to_string(),
        deathstate: f[12].trim_start_matches("S_").to_string(),
        xdeathstate: f[13].trim_start_matches("S_").to_string(),
        deathsound: f[14].trim_start_matches("sfx_").to_string(),
        speed: f[15].parse().unwrap_or(0),
        radius: f[16].parse().unwrap_or(0),
        height: f[17].parse().unwrap_or(0),
        mass: f[18].parse().unwrap_or(0),
        damage: f[19].parse().unwrap_or(0),
        activesound: f[20].trim_start_matches("sfx_").to_string(),
        flags: f[21].to_string(),
        raisestate: f[22].trim_start_matches("S_").to_string(),
    })
}

// ---------------------------------------------------------------
// Render Rust constants
// ---------------------------------------------------------------

fn render_states(rows: &[StateRow]) -> String {
    let mut out = String::from(
        "// AUTO-GENERATED - see tools/gen_mobjinfo\n\n\
use crate::defs::state::State;\n\
use crate::defs::action::Action;\n\n\
#[derive(Debug, Copy, Clone)]\n\
pub struct StateInfo {\n\
    pub state: State,\n\
    pub sprite: &'static str,\n\
    pub frame: u8,\n\
    pub tics: i32,\n\
    pub action: Action,\n\
    pub next_state: State,\n\
    pub misc1: i32,\n\
    pub misc2: i32,\n\
}\n\n\
pub const STATES: &[StateInfo] = &[\n",
    );

    for s in rows.iter() {
        let action_token = if s.action == "NULL" {
            "Action::None".to_string()
        } else {
            format!("Action::{}", s.action)
        };

        out.push_str(&format!(
            "   StateInfo {{ state: State::{}, sprite: \"{}\", frame: {}, tics: {}, \
             action: {}, next_state: State::{}, misc1: {}, misc2: {} }},\n",
            s.name, s.sprite, s.frame, s.tics, action_token, s.next_state, s.misc1, s.misc2,
        ));
    }
    out.push_str("];");
    out
}

fn render_mobjinfo(rows: &[MobjRow]) -> String {
    let mut out = String::from(
        "// AUTO-GENERATED - see tools/gen_mobjinfo\n\n\
use crate::defs::flags::MobjFlags as MF;\n\
use crate::defs::{state::State, sound::Sound};\n\n\
#[derive(Debug, Clone)]\n\
pub struct MobjInfo {\n\
    pub id: &'static str,\n\
    pub doomednum:    i32,\n\
    /* state chain */\n\
    pub spawnstate:   State,\n\
    pub spawnhealth:  i32,\n\
    pub seestate:     State,\n\
    pub seesound:     Sound,\n\
    pub reactiontime: i32,\n\
    pub attacksound:  Sound,\n\
    pub painstate:    State,\n\
    pub painchance:   i32,\n\
    pub painsound:    Sound,\n\
    pub meleestate:   State,\n\
    pub missilestate: State,\n\
    pub deathstate:   State,\n\
    pub xdeathstate:  State,\n\
    pub deathsound:   Sound,\n\
    /* physics & damage */\n\
    pub speed:        i32,\n\
    pub radius:       i32,\n\
    pub height:       i32,\n\
    pub mass:         i32,\n\
    pub damage:       i32,\n\
    /* ambience & behaviour */\n\
    pub activesound:  Sound,\n\
    pub flags:        MF,\n\
    pub raisestate:   State,\n\
}\n\n\
pub const MOBJINFO: &[MobjInfo] = &[\n",
    );

    let quote = |s: &str| format!("\"{}\"", s);

    let fmt_state = |raw: &str| {
        if raw == "0" {
            "State::NULL".into()
        } else {
            format!("State::{}", raw)
        }
    };

    let fmt_sound = |raw: &str| {
        if raw == "0" {
            "Sound::None".into()
        } else {
            format!("Sound::{}", raw)
        }
    };

    let fmt_flags = |raw: &str| {
        let trimmed = raw.trim();
        if trimmed == "0" || trimmed.is_empty() {
            return "MF::empty()".into();
        }

        let bits = raw
            .split('|')
            .map(|tok| format!("MF::{}.bits()", tok.trim().trim_start_matches("MF_")))
            .collect::<Vec<_>>()
            .join(" | ");

        format!("MF::from_bits_truncate({bits})")
    };

    for m in rows.iter() {
        out.push_str(&format!(
            "MobjInfo {{ \
id: {id}, \
doomednum: {dn}, \
spawnstate: {ss}, \
spawnhealth: {sh}, \
seestate: {se}, \
seesound: {snd}, \
reactiontime: {rt}, \
attacksound: {atk}, \
painstate: {ps}, \
painchance: {pc}, \
painsound: {pns}, \
meleestate: {me}, \
missilestate: {ms}, \
deathstate: {ds}, \
xdeathstate: {xds}, \
deathsound: {dths}, \
speed: {spd}, \
radius: {rad}, \
height: {hgt}, \
mass: {mss}, \
damage: {dmg}, \
activesound: {acts}, \
flags: {flg}, \
raisestate: {rs} }},\n",
            id = quote(&m.id),
            dn = m.doomednum,
            ss = fmt_state(&m.spawnstate),
            sh = m.spawnhealth,
            se = fmt_state(&m.seestate),
            snd = fmt_sound(&m.seesound),
            rt = m.reactiontime,
            atk = fmt_sound(&m.attacksound),
            ps = fmt_state(&m.painstate),
            pc = m.painchance,
            pns = fmt_sound(&m.painsound),
            me = fmt_state(&m.meleestate), // ← 0 ➜ State::S_NULL
            ms = fmt_state(&m.missilestate),
            ds = fmt_state(&m.deathstate),
            xds = fmt_state(&m.xdeathstate),
            dths = fmt_sound(&m.deathsound),
            spd = m.speed,
            rad = m.radius,
            hgt = m.height,
            mss = m.mass,
            dmg = m.damage,
            acts = fmt_sound(&m.activesound),
            flg = fmt_flags(&m.flags),
            rs = fmt_state(&m.raisestate),
        ));
    }

    out.push_str("];\n");
    out
}

// ---------------------------------------------------------------
//  state.rs generator
// ---------------------------------------------------------------

fn render_state(rows: &[StateRow]) -> String {
    let mut out = String::from(
        "// AUTO-GENERATED - see tools/gen_mobjinfo\n\n\
#[repr(usize)]\n\
#[derive(Debug, Copy, Clone, PartialEq, Eq)]\n\
#[allow(non_camel_case_types)]\n\
pub enum State {\n",
    );

    for (i, row) in rows.iter().enumerate() {
        // fall back to S_IDXnnn if the comment was missing
        let name = if row.name.is_empty() {
            format!("S_IDX{:03}", i)
        } else {
            row.name.clone()
        };
        out.push_str(&format!("    {} = {},\n", name, i));
    }

    out.push_str("}\n\n");

    out.push_str(
        "impl State {\n\
#[inline(always)]\n\
pub fn info(self) -> &'static super::states::StateInfo {\n\
    &super::states::STATES[self as usize]\n\
}\n\
#[inline(always)]\n\
pub fn tics(self) -> i32 {\n\
    self.info().tics\n\
}\n\
#[inline(always)]\n\
pub fn next(self) -> State {\n\
    self.info().next_state\n\
}\n\
#[inline(always)]\n\
pub fn sprite(self) -> &'static str {\n\
    self.info().sprite\n\
}\n\
#[inline(always)]\n\
pub fn frame(self) -> u8 {\n\
    self.info().frame\n\
}\n\
}\n",
    );

    out
}

fn render_action(names: &[String]) -> String {
    let mut out = String::from(
        "// AUTO-GENERATED - see tools/gen_mobjinfo\n\n\
#[derive(Debug, Copy, Clone)]\n\
#[allow(non_camel_case_types)]\n\
pub enum Action {\n    None,\n",
    );
    for n in names {
        out.push_str(&format!("    {},\n", n));
    }
    out.push_str("}\n");
    out
}

fn render_sound(names: &[String]) -> String {
    let mut out = String::from(
        "// AUTO-GENERATED - see tools/gen_mobjinfo\n\n\
#[derive(Debug, Copy, Clone)]\n\
#[allow(non_camel_case_types)]\n\
pub enum Sound {\n",
    );
    for n in names {
        if n != "0" {
            out.push_str(&format!("    {},\n", n));
        }
    }
    out.push_str("}\n");
    out
}
