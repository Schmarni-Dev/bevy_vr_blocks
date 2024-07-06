use avian3d::dynamics::rigid_body::{AngularVelocity, LinearVelocity};
use bevy::prelude::*;
use bevy_mod_openxr::{
    features::handtracking::{spawn_hand_bones, OxrHandTracker}, helper_traits::ToVec3, init::create_xr_session, resources::{OxrFrameState, Pipelined}, session::OxrSession
};
use bevy_mod_xr::{
    hands::{HandBone, HandBoneRadius, LeftHand, RightHand, XrHandBoneEntities},
    session::{session_running, XrCreateSession, XrDestroySession, XrTrackingRoot},
    spaces::{XrPrimaryReferenceSpace, XrReferenceSpace},
};
use openxr::{SpaceLocationFlags, SpaceVelocityFlags};

#[derive(Clone, Copy, Component, Default)]
pub struct XrVelocity {
    pub linear: Vec3,
    pub angular: Vec3,
}

pub struct CustomHandTrackingPlugin;
#[derive(Clone, Copy, Component)]
pub struct CustomHandBone;
#[derive(Clone, Copy, Component)]
pub struct CustomHandTracker;

impl Plugin for CustomHandTrackingPlugin {
    fn build(&self, app: &mut App) {
        // This might crash on bevy_mod_xr 0.1.0-rc1 because of scheduling, sorry for not catching
        // that - by Schmarni
        app.add_systems(XrCreateSession, spawn_custom_hands.after(create_xr_session));
        app.add_systems(XrDestroySession, clean_up_custom_hands);
        app.add_systems(
            PreUpdate,
            (locate_hands_with_vel, transfer_vels)
                .chain()
                .run_if(session_running),
        );
    }
}
fn transfer_vels(mut query: Query<(&XrVelocity, &mut LinearVelocity, &mut AngularVelocity)>) {
    for (vel, mut linear_vel, mut angular_vel) in &mut query {
        **linear_vel = vel.linear;
        **angular_vel = vel.angular;
    }
}

fn locate_hands_with_vel(
    default_ref_space: Res<XrPrimaryReferenceSpace>,
    frame_state: Res<OxrFrameState>,
    tracker_query: Query<(
        &OxrHandTracker,
        Option<&XrReferenceSpace>,
        &XrHandBoneEntities,
    )>,
    session: Res<OxrSession>,
    mut bone_query: Query<(
        &HandBone,
        &mut HandBoneRadius,
        &mut Transform,
        &mut XrVelocity,
    )>,
    pipelined: Option<Res<Pipelined>>,
) {
    for (tracker, ref_space, hand_entities) in &tracker_query {
        let ref_space = ref_space.map(|v| &v.0).unwrap_or(&default_ref_space.0);
        // relate_hand_joints also provides velocities
        let joints = match session.locate_hand_joints_with_velocities(
            tracker,
            ref_space,
            if pipelined.is_some() {
                openxr::Time::from_nanos(
                    frame_state.predicted_display_time.as_nanos()
                        + frame_state.predicted_display_period.as_nanos(),
                )
            } else {
                frame_state.predicted_display_time
            },
        ) {
            Ok(Some(v)) => v,
            Ok(None) => continue,
            Err(openxr::sys::Result::ERROR_EXTENSION_NOT_PRESENT) => {
                error!("HandTracking Extension not loaded");
                continue;
            }
            Err(err) => {
                warn!("Error while locating hand joints: {}", err.to_string());
                continue;
            }
        };
        let bone_entities = match bone_query.get_many_mut(hand_entities.0) {
            Ok(v) => v,
            Err(err) => {
                warn!("unable to get entities, {}", err);
                continue;
            }
        };
        for (bone, mut bone_radius, mut transform, mut vel) in bone_entities {
            let joint = joints.0[*bone as usize];
            let joint_vel = joints.1[*bone as usize];
            **bone_radius = joint.radius;
            if joint_vel
                .velocity_flags
                .contains(SpaceVelocityFlags::LINEAR_VALID)
            {
                vel.linear = joint_vel.linear_velocity.to_vec3();
            } else {
                vel.linear = Vec3::ZERO;
            }
            if joint_vel
                .velocity_flags
                .contains(SpaceVelocityFlags::ANGULAR_VALID)
            {
                vel.angular = joint_vel.angular_velocity.to_vec3();
            } else {
                vel.angular = Vec3::ZERO;
            }
            if joint
                .location_flags
                .contains(SpaceLocationFlags::POSITION_VALID)
            {
                transform.translation.x = joint.pose.position.x;
                transform.translation.y = joint.pose.position.y;
                transform.translation.z = joint.pose.position.z;
            }

            if joint
                .location_flags
                .contains(SpaceLocationFlags::ORIENTATION_VALID)
            {
                transform.rotation.x = joint.pose.orientation.x;
                transform.rotation.y = joint.pose.orientation.y;
                transform.rotation.z = joint.pose.orientation.z;
                transform.rotation.w = joint.pose.orientation.w;
            }
        }
    }
}

fn spawn_custom_hands(
    mut cmds: Commands,
    session: Res<OxrSession>,
    root: Query<Entity, With<XrTrackingRoot>>,
) {
    debug!("spawning default hands");
    let Ok(root) = root.get_single() else {
        error!("unable to get tracking root, skipping hand creation");
        return;
    };
    let tracker_left = match session.create_hand_tracker(openxr::HandEXT::LEFT) {
        Ok(t) => t,
        Err(openxr::sys::Result::ERROR_EXTENSION_NOT_PRESENT) => {
            warn!("Handtracking Extension not loaded, Unable to create Handtracker!");
            return;
        }
        Err(err) => {
            warn!("Error while creating Handtracker: {}", err.to_string());
            return;
        }
    };
    let tracker_right = match session.create_hand_tracker(openxr::HandEXT::RIGHT) {
        Ok(t) => t,
        Err(openxr::sys::Result::ERROR_EXTENSION_NOT_PRESENT) => {
            warn!("Handtracking Extension not loaded, Unable to create Handtracker!");
            return;
        }
        Err(err) => {
            warn!("Error while creating Handtracker: {}", err.to_string());
            return;
        }
    };
    let left_bones = spawn_hand_bones(&mut cmds, (CustomHandBone, LeftHand, XrVelocity::default()));
    let right_bones = spawn_hand_bones(
        &mut cmds,
        (CustomHandBone, RightHand, XrVelocity::default()),
    );
    cmds.entity(root).push_children(&left_bones);
    cmds.entity(root).push_children(&right_bones);
    cmds.spawn((
        CustomHandTracker,
        OxrHandTracker(tracker_left),
        XrHandBoneEntities(left_bones),
        LeftHand,
    ));
    cmds.spawn((
        CustomHandTracker,
        OxrHandTracker(tracker_right),
        XrHandBoneEntities(right_bones),
        RightHand,
    ));
}
#[allow(clippy::type_complexity)]
fn clean_up_custom_hands(
    mut cmds: Commands,
    query: Query<Entity, Or<(With<CustomHandTracker>, With<CustomHandBone>)>>,
) {
    for e in &query {
        debug!("removing default hand entity");
        cmds.entity(e).despawn_recursive();
    }
}
