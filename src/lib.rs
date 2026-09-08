// Combat Shield plugin for SC:R via Samase.
//
// Design (see chat for full reasoning): BW has no per-unit "max HP" field at all -
// max HP is always looked up live from the unit's *type* via units.dat. So instead
// of trying to patch a max_hitpoints field that doesn't exist, Combat Shield works
// by switching a player's Marines to a second units.dat entry ("Shielded Marine")
// that already has 55 HP baked in - the same technique mtl uses for set_unit_id
// (see mtl's src/unit.rs UnitExt::set_unit_id, which this borrows from directly).
//
// You still need to do the PyDAT step:
//   1. Clone Marine (unit id 0) into an unused unit id slot, call it "Shielded Marine".
//   2. Set that new entry's Hit Points to 55 (same dimensions/flags as Marine).
//   3. Set SHIELDED_MARINE_ID below to that unit id.
//   4. Keep Upgrade ID 55 = Combat Shield from the original guide (unchanged).

#![allow(non_upper_case_globals)]

use std::ffi::c_void;
use std::ptr::null_mut;

use bw_dat::{Game, Unit, UnitId, UpgradeId};

// --- Configure these two to match your PyDAT setup ---
const MARINE_ID: UnitId = UnitId(0);
const SHIELDED_MARINE_ID: UnitId = UnitId(92); // Shielded Marine (55 HP), created in PyDAT
const COMBAT_SHIELD_UPGRADE: UpgradeId = UpgradeId(55);
// -------------------------------------------------------

unsafe extern "C" fn frame_hook() {
    let mut unit_ptr = samase_first_active_unit();
    while !unit_ptr.is_null() {
        if let Some(unit) = Unit::from_ptr(unit_ptr as *mut bw_dat::structs::Unit) {
            apply_combat_shield(unit);
        }
        unit_ptr = (*(unit_ptr as *mut bw_dat::structs::Unit)).flingy.next as *mut c_void;
    }
}

// --- Temporary diagnostics: print once per checkpoint so we can see exactly
// where the logic stops working, without spamming every frame for every unit.
static mut DBG_SAW_MARINE: bool = false;
static mut DBG_GAME_NULL: bool = false;
static mut DBG_UPGRADE_LEVEL: bool = false;

unsafe fn apply_combat_shield(unit: Unit) {
    if unit.id() != MARINE_ID {
        return;
    }
    if !DBG_SAW_MARINE {
        DBG_SAW_MARINE = true;
        let msg = format!("Combat Shield DEBUG: saw a Marine (id={})\0", unit.id().0);
        samase_print_text(msg.as_ptr());
    }
    let game_ptr = samase_game();
    if game_ptr.is_null() {
        if !DBG_GAME_NULL {
            DBG_GAME_NULL = true;
            samase_print_text(b"Combat Shield DEBUG: game_ptr is null\0".as_ptr());
        }
        return;
    }
    let game = Game::from_ptr(game_ptr as *mut bw_dat::structs::Game);
    let level = game.upgrade_level(unit.player(), COMBAT_SHIELD_UPGRADE);
    if level > 0 {
        let msg = format!(
            "Combat Shield DEBUG: upgrade_level for player {} = {}\0",
            unit.player(), level
        );
        samase_print_text(msg.as_ptr());
    } else if !DBG_UPGRADE_LEVEL {
        DBG_UPGRADE_LEVEL = true;
        let msg = format!(
            "Combat Shield DEBUG: upgrade_level for player {} = {} (pre-research)\0",
            unit.player(), level
        );
        samase_print_text(msg.as_ptr());
    }
    if level == 0 {
        return;
    }
    // Already swapped, nothing to do.
    if unit.id() == SHIELDED_MARINE_ID {
        return;
    }
    swap_unit_id(unit, SHIELDED_MARINE_ID);
}

// Adapted directly from mtl's UnitExt::set_unit_id (src/unit.rs) - proportionally
// scales current HP between the old and new dat max, same as mtl does for its own
// set_unit_id ini feature, rather than inventing new scaling logic.
unsafe fn swap_unit_id(unit: Unit, new: UnitId) {
    let old = unit.id();
    let old_max = old.hitpoints() >> 8;
    let new_max = new.hitpoints() >> 8;
    let current_hp = (unit.hitpoints() >> 8).max(1);

    // mtl's own set_unit_id (src/unit.rs) gets the raw pointer via `*self`, not a
    // named accessor - Unit derefs directly to *mut bw_dat::structs::Unit.
    let raw: *mut bw_dat::structs::Unit = *unit;
    (*raw).unit_id = new.0;
    let new_hp = new_max
        .saturating_mul(current_hp)
        .checked_div(old_max)
        .unwrap_or(1)
        .max(1);
    (*raw).flingy.hitpoints = new_hp << 8;

    // Force a redraw in case the wireframe/sprite color is cached rather than
    // recomputed live from HP - mirrors mtl's own redraw call (src/frame_hook
    // area), even though mtl only does this on classic engine. Cheap to try
    // since we're specifically testing SD/Retro rendering here.
    if let Some(sprite) = unit.sprite() {
        for image in sprite.images() {
            image.redraw();
        }
    }

    // DEBUG: print the real raw values driving this swap, to check whether the
    // wireframe color mismatch is a data issue (e.g. new_hp/new_max not actually
    // reaching 1.0) rather than an asset issue - remove once diagnosed.
    let msg = format!(
        "Combat Shield swap: old_max={} new_max={} current_hp={} -> new_hp={} (raw hitpoints field={})\0",
        old_max, new_max, current_hp, new_hp, (*raw).flingy.hitpoints
    );
    samase_print_text(msg.as_ptr());
}

// --- Minimal raw bindings to the pieces of PluginApi we actually need ---
// (samase_plugin crate supplies the real, verified PluginApi type - we just
// stash the few function pointers we call every frame.)

static mut GET_GAME_FN: Option<unsafe extern "C" fn() -> *mut c_void> = None;
static mut FIRST_ACTIVE_UNIT_FN: Option<unsafe extern "C" fn() -> *mut c_void> = None;
static mut PRINT_TEXT_FN: Option<unsafe extern "C" fn(*const u8)> = None;

unsafe fn samase_print_text(msg: *const u8) {
    if let Some(f) = PRINT_TEXT_FN {
        f(msg);
    }
}

unsafe fn samase_first_active_unit() -> *mut c_void {
    match FIRST_ACTIVE_UNIT_FN {
        Some(f) => f(),
        None => null_mut(),
    }
}

// Called fresh each time, not cached at init - at plugin-init time (DLL load,
// main menu) there is no game yet, so caching the *result* once would freeze
// in a stale/null pointer. We cache the getter *function*, and call it fresh
// every time we actually need the current game, same pattern as first_active_unit.
unsafe fn samase_game() -> *mut c_void {
    match GET_GAME_FN {
        Some(f) => f(),
        None => null_mut(),
    }
}

// units.dat is required for Unit::id().hitpoints() to resolve correctly.
// Must be called synchronously inside samase_plugin_init itself - PluginApi
// functions like extended_dat are only valid to call during init, on the
// init thread (calling them later, e.g. from a hook callback, panics inside
// Samase itself: "Plugin api function called after plugin init or from
// different thread"). mtl calls this the same way, directly in its own
// samase_plugin_init, not deferred - confirmed by re-reading its source.
unsafe fn init_units_dat(api: &samase_plugin::PluginApi) {
    let mut dat_len = 0usize;
    if let Some(get_units_dat) = (api.extended_dat)(0) {
        let units_dat = get_units_dat(&mut dat_len);
        bw_dat::init_units(units_dat as *const _, dat_len);
    } else {
        (api.crash_with_message)(b"Combat Shield: units.dat unavailable\0".as_ptr());
    }
}

// TEST: upgrade_level() started always returning 0 after we bumped bw_dat to a
// newer revision (done to try to fix Tatti-format units.dat compatibility).
// Theory: the newer revision may now require its own explicit init - mirroring
// init_units above - before upgrade_level() can correctly determine extended
// upgrade layout/stride, defaulting to 0 without it. extended_dat(3) = upgrades,
// per mtl's own dat-index convention (0=units,1=weapons,2=flingy,3=upgrades).
// SUPERSEDED: this was a wrong guess (crashed init) at fixing extended-dat
// upgrade reading. The real fix, per neivv directly, is bw_dat::set_extended_arrays
// in samase_plugin_init (see above) - keeping this here only for the record.

#[no_mangle]
pub unsafe extern "C" fn samase_plugin_init(api: *const samase_plugin::PluginApi) {
    let api = &*api;

    // Real fix per neivv's own guidance for extended-format dat compatibility
    // (e.g. Tatti-saved units.dat/upgrades.dat) - confirmed against mtl's actual
    // source, same real call, not a guess this time.
    bw_dat::set_is_scr(true);
    let mut ext_arrays = std::ptr::null_mut();
    let ext_arrays_len = (api.extended_arrays)(&mut ext_arrays);
    bw_dat::set_extended_arrays(ext_arrays as *mut _, ext_arrays_len);

    init_units_dat(api);

    if let Some(get_game) = (api.game)() {
        GET_GAME_FN = Some(get_game);
    }
    if let Some(get_first_active) = (api.first_active_unit)() {
        FIRST_ACTIVE_UNIT_FN = Some(get_first_active);
    }
    if let Some(get_print_text) = (api.print_text)() {
        PRINT_TEXT_FN = Some(get_print_text);
    }

    let result = (api.hook_step_objects)(frame_hook, 0);
    if result == 0 {
        (api.crash_with_message)(b"Combat Shield: couldn't hook step_objects\0".as_ptr());
    }
}