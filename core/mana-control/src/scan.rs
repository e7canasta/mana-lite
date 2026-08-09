//! Fixed-cadence scene decisions over a frozen process image.
use std::time::{Duration, Instant};

pub use crate::{AgedEvidence, ProcessImage, SceneObservation, SceneSample};
pub use crate::SceneSample as ClinicalSample;
use crate::fsm::{FsmEngine,FsmSceneContext,FsmTransitionResult};
use crate::health::{Health,HealthTransition};
use crate::occupancy::{self, OccupancyStateMachine,RoomCardinality,SecondPersonState,SignalValidity};
use crate::presence::{PresenceFilter,PresenceState};
use crate::track::{Track,Tracker,TrackEvent};
use crate::zones::{ZoneEngine,ZoneEvent};
#[derive(Debug,Clone,Copy,PartialEq,Eq)] pub struct ScanInstant(Instant);
impl ScanInstant {
    // FIXME(ADR-029): wall-clock escape hatch; ScanInstant must only be born from ScanTimeline.
    pub fn now()->Self{Self(Instant::now())}
    pub const fn from_instant(v:Instant)->Self{Self(v)}
    pub const fn as_instant(self)->Instant{self.0}
    pub fn elapsed_ms_since(self, earlier: Self)->u64 {
        self.0.saturating_duration_since(earlier.0).as_millis() as u64
    }
}
#[derive(Debug,Clone,Copy,PartialEq,Eq)] pub struct ControlStamp { pub scan_seq:u64,pub evidence_frame_id:u64,pub observations_age_ms:u64,pub depth_age_ms:Option<u64> }
#[derive(Debug,Clone,Copy,PartialEq,Eq)] pub struct ScanConfig { pub period_ms:u64 }
impl Default for ScanConfig { fn default()->Self{Self{period_ms:200}} }
impl ScanConfig { pub fn period(self)->Duration{Duration::from_millis(self.period_ms.max(1))} }
#[derive(Debug,Clone)] pub struct ControlPolicy { pub person_class:String,pub presence_enabled:bool,pub data_stale_ms:u64,pub scan_period_ms:u64,pub face_dwell_roi:Option<[u32;4]>,pub person_detection_roi:Option<[u32;4]>,pub face_edge_margin_px:u32 }
pub struct ControlState { pub tracker:Option<Tracker>,pub presence:PresenceFilter,pub occupancy:OccupancyStateMachine,pub zone_engine:Option<ZoneEngine>,pub fsm_engine:Option<FsmEngine>,pub health:Health,pub fsm_context:FsmSceneContext,pub last_scan_at:Instant,pub scan_seq:u64,pub policy:ControlPolicy }
#[derive(Debug,Clone)] pub enum SceneEvent { Track{event:TrackEvent,stamp:ControlStamp}, Presence{stamp:ControlStamp,state:RoomCardinality,presence:PresenceState,second_person:SecondPersonState,signal:SignalValidity,raw_person_count:usize,confirmed_person_count:usize,held:bool,positive_ms:u64,empty_ms:u64,single_timer_ms:u64,empty_timer_ms:u64,multiple_candidate_timer_ms:u64,multiple_exit_timer_ms:u64}, Occupancy{state:RoomCardinality,second_person:SecondPersonState,signal:SignalValidity}, EntityBoxes(Vec<Track>), Zone{event:ZoneEvent,stamp:ControlStamp}, FsmTransition(FsmTransitionResult), FsmState(String), Health(HealthTransition) }
pub fn scan(state:&mut ControlState,image:&ProcessImage,now:ScanInstant)->Vec<SceneEvent>{let now=now.as_instant();let dt=now.saturating_duration_since(state.last_scan_at).as_millis() as u64;let dt=dt.max(state.policy.scan_period_ms.max(1));state.last_scan_at=now;state.scan_seq=state.scan_seq.saturating_add(1);if let Some(t)=state.tracker.as_mut(){t.predict_at(dt)} let age=image.observations_age_ms(now);let sample=image.observations.as_ref().map(|x|x.value.clone()).unwrap_or_else(SceneSample::unavailable);let stamp=ControlStamp{scan_seq:state.scan_seq,evidence_frame_id:sample.frame_number,observations_age_ms:age,depth_age_ms:image.depth_age_ms(now)};let valid=sample.signal_valid&&age<=state.policy.data_stale_ms;let (observations,presence)=state.presence.update_at(&sample.observations,valid,now);let tracking=if presence.state==PresenceState::Ambiguous {&[][..]} else {&observations};let mut events=Vec::new();if let Some(t)=state.tracker.as_mut(){let allow=sample.raw_person_count==1&&tracking.len()==1&&tracking[0].class==state.policy.person_class;let input=if image.measurement_pending{tracking}else{&[]};for event in t.associate_observations(input,allow&&image.measurement_pending,dt){events.push(SceneEvent::Track{event,stamp})}}let confirmed=state.tracker.as_ref().map_or(0,|t|t.current_tracks().into_iter().filter(|x|x.class==state.policy.person_class).count());let update=state.occupancy.update_at(occupancy::build_evidence(occupancy::OccupancyEvidenceInputs{tracking_enabled:state.tracker.is_some(),presence_enabled:state.policy.presence_enabled,signal_valid:valid,raw_person_count:sample.raw_person_count,confirmed_person_count:confirmed,presence_is_present:presence.state==PresenceState::Present,presence_held:presence.held}),now);events.push(SceneEvent::Occupancy{state:update.state,second_person:update.second_person,signal:SignalValidity::from_bool(valid)});events.push(SceneEvent::Presence{stamp,state:update.state,presence:presence.state,second_person:update.second_person,signal:SignalValidity::from_bool(valid),raw_person_count:sample.raw_person_count,confirmed_person_count:confirmed,held:presence.held,positive_ms:presence.positive_ms,empty_ms:presence.empty_ms,single_timer_ms:update.single_timer_ms,empty_timer_ms:update.empty_timer_ms,multiple_candidate_timer_ms:update.multiple_candidate_timer_ms,multiple_exit_timer_ms:update.multiple_exit_timer_ms});update_context(&mut state.fsm_context,&state.policy,update.state,sample.raw_person_count,sample.face_model_ran,&sample.observations);let zones=if let Some(z)=state.zone_engine.as_mut(){let tracks=state.tracker.as_ref().map_or_else(Vec::new,|t|t.current_tracks());let es=z.evaluate_at(&tracks,now);for event in &es{events.push(SceneEvent::Zone{event:event.clone(),stamp})}es}else{Vec::new()};if let Some(t)=state.tracker.as_ref(){events.push(SceneEvent::EntityBoxes(t.current_tracks().into_iter().cloned().collect()))}if let Some(f)=state.fsm_engine.as_mut(){let depth=image.depth_snapshot();if let Some(x)=f.evaluate_with_context_at(&zones,state.zone_engine.as_ref(),&state.health,&depth,&state.fsm_context,now){events.push(SceneEvent::FsmTransition(x))}events.push(SceneEvent::FsmState(f.snapshot_at(now).state));if let Some(x)=f.evaluate_wildcard_with_context_at(&[],state.zone_engine.as_ref(),&state.health,&depth,&state.fsm_context,now){events.push(SceneEvent::FsmTransition(x));events.push(SceneEvent::FsmState(f.snapshot_at(now).state))}}let h=state.health.evaluate_at(now);if !matches!(h,HealthTransition::None){events.push(SceneEvent::Health(h))}events}
fn update_context(ctx:&mut FsmSceneContext,policy:&ControlPolicy,cardinality:RoomCardinality,raw:usize,face_model_ran:bool,obs:&[SceneObservation]){let person=obs.iter().filter(|x|x.class==policy.person_class).max_by(|a,b|a.confidence.total_cmp(&b.confidence));let face=person.and_then(|x|x.face);ctx.cardinality=Some(cardinality.as_str().into());ctx.person_present=raw>0;ctx.face_present=face.is_some();ctx.face_confidence=face.map(|x|x.confidence);ctx.face_in_dwell=policy.face_dwell_roi.map(|r|face.is_some_and(|f|intersects(f.bbox,r)));ctx.at_edge=person.is_some_and(|p|near(p.bbox,policy.person_detection_roi,policy.face_edge_margin_px));ctx.face_model_ran=face_model_ran}
fn intersects(b:[f32;4],r:[u32;4])->bool{b[0]<r[2]as f32&&b[2]>r[0]as f32&&b[1]<r[3]as f32&&b[3]>r[1]as f32}fn near(b:[f32;4],r:Option<[u32;4]>,m:u32)->bool{let Some(r)=r else{return false};let m=m as f32;b[0]<=r[0]as f32+m||b[1]<=r[1]as f32+m||b[2]>=r[2]as f32-m||b[3]>=r[3]as f32-m}
#[derive(Debug,Clone,Copy)] pub struct ScanTimeline{start:Instant,period_ms:u64,tick:u64}impl ScanTimeline{pub fn new(start:Instant,period_ms:u64)->Self{Self{start,period_ms:period_ms.max(1),tick:0}}pub fn now(self)->ScanInstant{ScanInstant::from_instant(self.start+Duration::from_millis(self.tick*self.period_ms))}pub fn advance(&mut self)->ScanInstant{self.tick+=1;self.now()}}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{OccupancyPolicy, PresencePoiPolicy};
    use std::time::Duration;

    fn person() -> SceneObservation {
        SceneObservation {
            class: "person".into(),
            bbox: [0.0, 0.0, 100.0, 100.0],
            confidence: 0.9,
            source_models: vec!["synthetic".into()],
            face: None,
        }
    }

    fn control_state(start: Instant, data_stale_ms: u64) -> ControlState {
        ControlState {
            tracker: None,
            presence: PresenceFilter::new(
                true,
                "person",
                PresencePoiPolicy {
                    on_ms: 0,
                    off_ms: 500,
                },
            ),
            occupancy: OccupancyStateMachine::new(OccupancyPolicy {
                single_confirm_ms: 0,
                empty_confirm_ms: 500,
                multiple_confirm_ms: 300,
                multiple_exit_ms: 300,
                require_confirmed_tracks: false,
            }),
            zone_engine: None,
            fsm_engine: None,
            health: Health::new_at(10_000, 5_000, start),
            fsm_context: Default::default(),
            last_scan_at: start,
            scan_seq: 0,
            policy: ControlPolicy {
                person_class: "person".into(),
                presence_enabled: true,
                data_stale_ms,
                scan_period_ms: 200,
                face_dwell_roi: None,
                person_detection_roi: None,
                face_edge_margin_px: 0,
            },
        }
    }

    #[test]
    fn scan_emits_presence_and_occupancy_on_valid_evidence() {
        let start = Instant::now();
        let mut timeline = ScanTimeline::new(start, 200);
        let mut state = control_state(start, 10_000);
        let image = ProcessImage {
            observations: Some(AgedEvidence::new(
                SceneSample {
                    observations: vec![person()],
                    signal_valid: true,
                    raw_person_count: 1,
                    frame_number: 1,
                    face_model_ran: false,
                },
                start,
            )),
            depth: None,
            measurement_pending: true,
        };

        let events = scan(&mut state, &image, timeline.now());
        assert!(
            events
                .iter()
                .any(|e| matches!(e, SceneEvent::Presence { .. })),
            "expected Presence event"
        );
        assert!(
            events
                .iter()
                .any(|e| matches!(e, SceneEvent::Occupancy { .. })),
            "expected Occupancy event"
        );
    }

    #[test]
    fn scan_marks_signal_invalid_when_evidence_is_stale() {
        let start = Instant::now();
        let data_stale_ms = 500;
        let mut state = control_state(start, data_stale_ms);
        let image = ProcessImage {
            observations: Some(AgedEvidence::new(
                SceneSample {
                    observations: vec![person()],
                    signal_valid: true,
                    raw_person_count: 1,
                    frame_number: 1,
                    face_model_ran: false,
                },
                start,
            )),
            depth: None,
            measurement_pending: true,
        };

        // Age beyond data_stale_ms while keeping the aged evidence frozen.
        let now = ScanInstant::from_instant(start + Duration::from_millis(data_stale_ms + 1));
        let events = scan(&mut state, &image, now);

        let presence_invalid = events.iter().any(|e| {
            matches!(
                e,
                SceneEvent::Presence {
                    signal: SignalValidity::Invalid,
                    ..
                }
            )
        });
        let occupancy_invalid = events.iter().any(|e| {
            matches!(
                e,
                SceneEvent::Occupancy {
                    signal: SignalValidity::Invalid,
                    ..
                }
            )
        });
        assert!(presence_invalid, "stale evidence must mark Presence Invalid");
        assert!(occupancy_invalid, "stale evidence must mark Occupancy Invalid");
    }
}
