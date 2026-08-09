use std::collections::HashMap; use serde::Deserialize; use crate::fsm::FsmGuard;
#[derive(Debug,Clone,Deserialize)] pub struct FsmCatalog { pub fsm:FsmRoot }
#[derive(Debug,Clone,Deserialize)] pub struct FsmRoot { pub initial:String, pub states:HashMap<String,FsmState>, pub roles:FsmRoles, #[serde(default)] pub transitions:Vec<FsmTransition> }
#[derive(Debug,Clone,Deserialize)] pub struct FsmRoles { pub safe:String, pub reset:String }
#[derive(Debug,Clone,Deserialize)] pub struct FsmState { #[serde(default)] pub label:Option<String>, #[serde(default)] pub models:Vec<String>, #[serde(default)] pub dwell_min_ms:Option<u64>, #[serde(default)] pub face_inside:bool, #[serde(default)] pub face_inside_maybe:bool }
#[derive(Debug,Clone,Deserialize)] pub struct FsmTransition { pub from:String, pub to:String, #[serde(default)] pub guards:Vec<FsmGuard>, #[serde(default)] pub dwell:Option<String> }
#[derive(Debug,Default,Clone,Deserialize)] pub struct ZoneCatalog { #[serde(default)] pub zones:HashMap<String,ZoneSpec>, #[serde(default)] pub face_dwell:Option<ZoneSpec> }
#[derive(Debug,Clone,Deserialize)] pub struct ZoneSpec { pub x1:u32,pub y1:u32,pub x2:u32,pub y2:u32,#[serde(default)] pub label:Option<String>,#[serde(default="default_hysteresis")] pub hysteresis_ms:u64 } impl ZoneSpec { pub fn rect(&self)->[u32;4]{[self.x1,self.y1,self.x2,self.y2]} } fn default_hysteresis()->u64{500}
#[derive(Debug,Clone)] pub struct PresencePoiPolicy { pub on_ms:u64,pub off_ms:u64 }
#[derive(Debug,Clone)] pub struct OccupancyPolicy { pub single_confirm_ms:u64,pub empty_confirm_ms:u64,pub multiple_confirm_ms:u64,pub multiple_exit_ms:u64,pub require_confirmed_tracks:bool }
