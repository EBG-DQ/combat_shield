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

unsafe fn apply_combat_shield(unit: Unit) {
    if unit.id() != MARINE_ID {
        return;
    }
    let game_ptr = samase_game();
    if game_ptr.is_null() {
        return;
    }
    let game = Game::from_ptr(game_ptr as *mut bw_dat::structs::Game);
    if game.upgrade_level(unit.player(), COMBAT_SHIELD_UPGRADE) == 0 {
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
}

// --- Minimal raw bindings to the pieces of PluginApi we actually need ---
// (samase_plugin crate supplies the real, verified PluginApi type - we just
// stash the few function pointers we call every frame.)

static mut GET_GAME_FN: Option<unsafe extern "C" fn() -> *mut c_void> = None;
static mut FIRST_ACTIVE_UNIT_FN: Option<unsafe extern "C" fn() -> *mut c_void> = None;

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

#[no_mangle]
pub unsafe extern "C" fn samase_plugin_init(api: *const samase_plugin::PluginApi) {
    let api = &*api;

    init_units_dat(api);

    if let Some(get_game) = (api.game)() {
        GET_GAME_FN = Some(get_game);
    }
    if let Some(get_first_active) = (api.first_active_unit)() {
        FIRST_ACTIVE_UNIT_FN = Some(get_first_active);
    }

    let result = (api.hook_step_objects)(frame_hook, 0);
    if result == 0 {
        (api.crash_with_message)(b"Combat Shield: couldn't hook step_objects\0".as_ptr());
    }
}