use glam::{DVec3, Mat4, Vec3};

/// Этап 4: "Реализация камеры, которая вращается и перемещается."
pub struct Camera {
    /// Where the eye is in the world, in `f64`: see `view_proj_about`.
    pub position: DVec3,
    pub yaw: f32,   // radians, rotation around Y (world up)
    pub pitch: f32, // radians, clamped to avoid flipping over the pole
    pub fov_y_radians: f32,
    pub aspect: f32,
    pub z_near: f32,
    pub z_far: f32,
    /// A purely visual offset added to the eye when building the view
    /// matrix: the running bob and the hit shake.
    ///
    /// Deliberately not folded into `position`. `position` is what the
    /// break/place ray starts from and what feeds the transform sent to
    /// the server, and a camera that jitters a few centimetres every
    /// frame would jitter both -- turning a cosmetic effect into aim
    /// wander and a stream of movement updates for a player standing
    /// still.
    pub shake: Vec3,
    /// Visual-only angular offset, as (pitch, yaw, roll) in radians.
    ///
    /// **This is the part that is actually seen.** Moving the eye a few
    /// centimetres barely changes the image at all: terrain is metres
    /// away, so a translation that small is almost pure parallax and
    /// reads as nothing happening. Rotating the view by a fraction of a
    /// degree moves every pixel on screen. A shake built only from
    /// translation is a shake nobody notices, which is exactly how the
    /// first attempt at this failed.
    ///
    /// Kept apart from `yaw`/`pitch` for the same reason `shake` is kept
    /// apart from `position`: those two are aim, and aim is the player's.
    pub shake_angles: Vec3,
}

const PITCH_LIMIT: f32 = std::f32::consts::FRAC_PI_2 - 0.01;

impl Camera {
    pub fn new(position: DVec3, aspect: f32) -> Self {
        Self {
            position,
            yaw: -std::f32::consts::FRAC_PI_2, // face -Z initially
            pitch: 0.0,
            fov_y_radians: 70f32.to_radians(),
            aspect,
            z_near: 0.05,
            z_far: 1000.0,
            shake: Vec3::ZERO,
            shake_angles: Vec3::ZERO,
        }
    }

    /// Where the view is actually taken from, shake included.
    /// The eye as it is drawn, bob and all, in the world.
    pub fn eye(&self) -> DVec3 {
        self.position + self.shake.as_dvec3()
    }

    /// The eye as it is drawn, bob and all, measured from `origin`.
    ///
    /// **From the origin, never from zero.** The eye used to be an `f32` in
    /// the world, and a million blocks out that is a number with sixteenths
    /// between its neighbours: the frame's origin moved in whole blocks, but
    /// the eye measured from it still stepped by sixteenths as the player
    /// walked, and the whole picture shivered by that much against the
    /// player's own hand. The difference is taken in `f64` and only then
    /// narrowed, so what reaches the matrix is a few blocks at most.
    pub fn eye_from(&self, origin: Vec3) -> Vec3 {
        (self.position - origin.as_dvec3()).as_vec3() + self.shake
    }

    /// The direction the *view* faces, which is aim plus shake.
    ///
    /// `forward` stays clean: it is what the break/place ray follows, so
    /// a shaking camera must not move the crosshair's target.
    fn view_forward(&self) -> Vec3 {
        let yaw = self.yaw + self.shake_angles.y;
        let pitch = (self.pitch + self.shake_angles.x)
            .clamp(-PITCH_LIMIT, PITCH_LIMIT);
        Vec3::new(
            yaw.cos() * pitch.cos(),
            pitch.sin(),
            yaw.sin() * pitch.cos(),
        )
        .normalize()
    }

    /// World up, rolled about the view axis.
    ///
    /// Roll is what sells both effects. `look_to_rh` with a hard-coded
    /// `Vec3::Y` can express a camera that nods and turns but never one
    /// that tilts, and a tilt is most of what a stride and a blow to the
    /// head actually look like.
    fn view_up(&self) -> Vec3 {
        let roll = self.shake_angles.z;
        if roll.abs() < 1e-6 {
            return Vec3::Y;
        }
        let axis = self.view_forward();
        let (sin, cos) = roll.sin_cos();
        // Rodrigues' rotation of Y about the view direction.
        Vec3::Y * cos + axis.cross(Vec3::Y) * sin + axis * axis.dot(Vec3::Y) * (1.0 - cos)
    }

    pub fn forward(&self) -> Vec3 {
        Vec3::new(
            self.yaw.cos() * self.pitch.cos(),
            self.pitch.sin(),
            self.yaw.sin() * self.pitch.cos(),
        )
        .normalize()
    }

    /// Forward projected onto the XZ plane, for WASD movement that doesn't
    /// climb/dive just because the player is looking up or down.
    pub fn forward_horizontal(&self) -> Vec3 {
        Vec3::new(self.yaw.cos(), 0.0, self.yaw.sin()).normalize()
    }

    pub fn right_horizontal(&self) -> Vec3 {
        self.forward_horizontal().cross(Vec3::Y).normalize()
    }

    pub fn apply_mouse_delta(&mut self, dx: f32, dy: f32, sensitivity: f32) {
        self.yaw += dx * sensitivity;
        self.pitch = (self.pitch - dy * sensitivity).clamp(-PITCH_LIMIT, PITCH_LIMIT);
    }

    /// The same, with the world shifted so that `origin` sits at zero.
    ///
    /// **Everything drawn goes through this, and `origin` is never far
    /// from the camera.** A view matrix built around an eye a million
    /// blocks out spends its whole `f32` mantissa on the translation --
    /// and then subtracts numbers of that size from each other for every
    /// vertex, which is where the precision goes. Shifting first means
    /// the matrix only ever holds small numbers, and the terrain arrives
    /// already shifted to match (see `mesh::Vertex::instance_layout`).
    ///
    /// The shift is a whole number of blocks and changes rarely, so it
    /// is exact and nothing it is subtracted from loses a bit to it.
    pub fn view_proj_about(&self, origin: Vec3) -> Mat4 {
        let view = Mat4::look_to_rh(self.eye_from(origin), self.view_forward(), self.view_up());
        let proj = Mat4::perspective_rh(self.fov_y_radians, self.aspect, self.z_near, self.z_far);
        proj * view
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pitch_cannot_flip_over_the_pole() {
        let mut camera = Camera::new((Vec3::ZERO).as_dvec3(), 1.0);
        camera.apply_mouse_delta(0.0, -100_000.0, 0.01);
        assert!(camera.pitch < std::f32::consts::FRAC_PI_2);
        camera.apply_mouse_delta(0.0, 200_000.0, 0.01);
        assert!(camera.pitch > -std::f32::consts::FRAC_PI_2);
    }

    #[test]
    fn horizontal_forward_ignores_pitch() {
        let mut camera = Camera::new((Vec3::ZERO).as_dvec3(), 1.0);
        camera.pitch = 1.0;
        assert!(camera.forward_horizontal().y.abs() < 1e-6);
    }

    #[test]
    fn the_bob_moves_the_picture_and_never_the_ray_the_crosshair_casts() {
        // **The one property the whole of `logic::shake` is built
        // around.** `position` and `forward` are what
        // `physics::raycast_block` is given and what the melee reach is
        // measured along, and they are what the server is told; a bob
        // that reached either of them would be a cosmetic wobble turned
        // into aim wander, and a player standing still streaming
        // movement updates for ever.
        //
        // Stated here rather than in `shake` because this is where the
        // separation actually lives: `shake` could be perfect and one
        // line of `eye()` leaking into `forward()` would undo it.
        let mut camera = Camera::new((Vec3::new(3.0, 40.0, -7.0)).as_dvec3(), 1.6);
        camera.yaw = 0.7;
        camera.pitch = -0.2;
        let aim = camera.forward();
        let stood = camera.position;
        let picture = camera.view_proj_about(Vec3::ZERO);

        camera.shake = Vec3::new(0.06, -0.04, 0.02);
        camera.shake_angles = Vec3::new(0.01, 0.0, 0.024);

        assert_eq!(camera.position, stood, "the bob moved where the player is");
        assert_eq!(camera.forward(), aim, "the bob moved what the crosshair is on");
        assert_ne!(
            camera.eye(),
            stood,
            "the bob was applied to the aim instead of to the view"
        );
        assert_ne!(
            camera.view_proj_about(Vec3::ZERO),
            picture,
            "the bob changed nothing on screen, which is the other way to get this wrong"
        );
    }

    /// **The picture a long way out is the picture at home.** The eye was an
    /// `f32` in the world, and though the frame's origin moved in whole
    /// blocks, the eye measured from it still stepped by a sixteenth a
    /// million blocks out -- the whole world shivered against the hand while
    /// the player walked. The same eye, the same few blocks from the origin,
    /// has to build the same matrix bit for bit.
    #[test]
    fn the_view_far_from_zero_is_the_view_at_home() {
        let view = |base: DVec3| {
            let mut camera = Camera::new(base + DVec3::new(3.37, 41.62, -7.19), 1.6);
            camera.yaw = 0.7;
            camera.pitch = -0.2;
            camera.view_proj_about((base + DVec3::new(2.0, 40.0, -9.0)).as_vec3())
        };
        let home = view(DVec3::ZERO);
        for base in [DVec3::new(1_000_000.0, 0.0, -1_000_000.0), DVec3::new(-10_000_000.0, 0.0, 10_000_000.0)] {
            assert_eq!(view(base), home, "the view at {base} is not the view at home");
        }
    }
}
