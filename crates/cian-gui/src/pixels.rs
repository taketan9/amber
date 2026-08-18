//! Real pixels, over the cells.
//!
//! Everything cian draws goes through a grid of character cells, and a glyph
//! can never be bigger than the cell it sits in. That is the wall this crate
//! kept running into: a file-type icon gets clipped because its ink is wider
//! than one cell, a picture preview has to be spelled out in half-blocks, and a
//! Finder-style icon view cannot exist at all.
//!
//! So the window grows a second layer. `ratatui-wgpu` hands a post-processor
//! the finished text as a texture and asks it to put something on the screen;
//! this one puts the text down exactly as the default would — by delegating to
//! it, so the text cannot drift — and then draws textured quads on top.
//!
//! Drawing *over* rather than *under* is deliberate. Under would mean punching
//! transparent holes in the text layer, which means teaching cian's renderer
//! about transparency it has no concept of. Over is what terminal graphics
//! protocols do too: cian leaves an area blank and says "a picture goes here".
//!
//! The caller thinks in physical pixels and hands over [`Draw`]s each frame;
//! textures are uploaded once and kept under a caller-chosen id.

use std::collections::HashMap;

use ratatui_wgpu::shaders::AspectPreservingDefaultPostProcessor;
use ratatui_wgpu::wgpu::*;
use ratatui_wgpu::PostProcessor;

/// One picture, placed in physical pixels from the top-left of the surface.
#[derive(Clone, Copy, Debug)]
pub struct Draw {
    pub id: u64,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    /// How solid to draw it. `1.0` for an icon in place, less for the ghost
    /// that follows the pointer while a file is being dragged.
    pub alpha: f32,
}

/// `[x, y, u, v, alpha]`, positions already in clip space.
type Vertex = [f32; 5];

const VERTEX_BYTES: u64 = std::mem::size_of::<Vertex>() as u64;
/// Room for this many quads before the buffer is grown. Six vertices each.
const INITIAL_QUADS: u64 = 64;

struct Upload {
    id: u64,
    width: u32,
    height: u32,
    rgba: Vec<u8>,
}

pub struct PixelLayer {
    /// The text, drawn by the implementation that owns that job.
    ///
    /// The *aspect-preserving* one, and the difference is the whole reason
    /// this line has a comment. The text is composited into a texture whose
    /// size is the largest exact multiple of the character size that fits the
    /// window — and the ordinary blitter then stretches that texture over the
    /// whole window. A window 2400 pixels wide with an 11-pixel cell holds 218
    /// characters and 2398 pixels of text, so the last two pixel columns are
    /// invented: the sampler is `Nearest`, so it duplicates two columns of
    /// pixels, spread evenly across the width. Those two columns are duplicated
    /// on *every row*, which is exactly what was reported — the same columns
    /// smeared, at every font size, in every view. The library says so itself:
    /// "This will stretch characters if the render area size falls between
    /// multiples of the character size."
    ///
    /// This one blits one texel to one pixel and leaves the remainder alone.
    /// See `BACKDROP` for what is done with the remainder.
    text: AspectPreservingDefaultPostProcessor,

    /// Kept because `process` is handed a queue but never a device, and a
    /// picture arriving mid-session needs a texture made for it.
    device: Device,
    pipeline: RenderPipeline,
    texture_layout: BindGroupLayout,
    sampler: Sampler,
    uniforms: BindGroup,

    vertices: Buffer,
    capacity: u64,

    cache: HashMap<u64, BindGroup>,
    /// Uploads waiting for a queue to write them. See `device`.
    pending: Vec<Upload>,
    frame: Vec<Draw>,
    /// What was actually drawn last time, so `needs_update` can tell whether
    /// this frame's list has anything new to say.
    drawn: Vec<Draw>,
    surface: (f32, f32),
    /// Read once, at build time. Asking the environment is a syscall on
    /// Windows, and this is the innermost loop the program has.
    keylog: bool,
}

impl PartialEq for Draw {
    fn eq(
        &self,
        other: &Self,
    ) -> bool {
        self.id == other.id
            && self.x == other.x
            && self.y == other.y
            && self.w == other.w
            && self.h == other.h
            && self.alpha == other.alpha
    }
}

impl PixelLayer {
    /// Is this picture already on the GPU?
    ///
    /// The caller keeps its own map of what it has uploaded, so nothing asks
    /// this at the moment. It stays because "have you got this one" is the
    /// question any second caller would open with.
    #[allow(dead_code)]
    pub fn has(&self, id: u64) -> bool {
        self.cache.contains_key(&id) || self.pending.iter().any(|u| u.id == id)
    }

    /// Hand over a picture as tightly-packed RGBA8. Uploaded on the next frame.
    pub fn upload(&mut self, id: u64, width: u32, height: u32, rgba: Vec<u8>) {
        if width == 0 || height == 0 {
            return;
        }
        debug_assert_eq!(rgba.len(), (width * height * 4) as usize);
        self.pending.push(Upload { id, width, height, rgba });
    }

    /// Forget a picture. The next [`upload`](Self::upload) under the same id
    /// starts again from nothing.
    ///
    /// Nothing calls this yet — the cache is small and every id so far lives as
    /// long as the session. It is here because the moment thumbnails arrive
    /// there will be thousands of them, and a cache with no way out is a leak
    /// with a nicer name.
    #[allow(dead_code)]
    pub fn evict(&mut self, id: u64) {
        self.cache.remove(&id);
        self.pending.retain(|u| u.id != id);
    }

    /// What to draw this frame. Replaces whatever the last frame asked for —
    /// a picture that is no longer wanted simply stops being listed.
    pub fn set_frame(&mut self, draws: Vec<Draw>) {
        self.frame = draws;
    }

    /// Turn a rectangle in physical pixels into the six clip-space vertices of
    /// two triangles. Y runs down the screen and up the clip volume, so it is
    /// the one that flips.
    fn quad(&self, d: &Draw) -> [Vertex; 6] {
        let (sw, sh) = self.surface;
        let x0 = d.x / sw * 2.0 - 1.0;
        let x1 = (d.x + d.w) / sw * 2.0 - 1.0;
        let y0 = 1.0 - d.y / sh * 2.0;
        let y1 = 1.0 - (d.y + d.h) / sh * 2.0;
        let a = d.alpha;
        [
            [x0, y0, 0.0, 0.0, a],
            [x1, y0, 1.0, 0.0, a],
            [x0, y1, 0.0, 1.0, a],
            [x1, y0, 1.0, 0.0, a],
            [x1, y1, 1.0, 1.0, a],
            [x0, y1, 0.0, 1.0, a],
        ]
    }

    fn make_texture(&self, queue: &Queue, u: &Upload) -> BindGroup {
        let texture = self.device.create_texture(&TextureDescriptor {
            label: Some("cian picture"),
            size: Extent3d { width: u.width, height: u.height, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            // Unorm rather than UnormSrgb: the shader does the conversion, and
            // only when the surface needs it — the same rule the text follows.
            format: TextureFormat::Rgba8Unorm,
            usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
            view_formats: &[],
        });
        queue.write_texture(
            TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: Origin3d::ZERO,
                aspect: TextureAspect::All,
            },
            &u.rgba,
            TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(u.width * 4),
                rows_per_image: Some(u.height),
            },
            Extent3d { width: u.width, height: u.height, depth_or_array_layers: 1 },
        );
        let view = texture.create_view(&TextureViewDescriptor::default());
        self.device.create_bind_group(&BindGroupDescriptor {
            label: Some("cian picture bindings"),
            layout: &self.texture_layout,
            entries: &[
                BindGroupEntry { binding: 0, resource: BindingResource::TextureView(&view) },
                BindGroupEntry { binding: 1, resource: BindingResource::Sampler(&self.sampler) },
            ],
        })
    }
}

impl PostProcessor for PixelLayer {
    type UserData = ();

    fn compile(
        device: &Device,
        text_view: &TextureView,
        surface_config: &SurfaceConfiguration,
        _user_data: Self::UserData,
    ) -> Self {
        let text = AspectPreservingDefaultPostProcessor::compile(
            device,
            text_view,
            surface_config,
            (),
        );

        let texture_layout = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: Some("cian picture layout"),
            entries: &[
                BindGroupLayoutEntry {
                    binding: 0,
                    visibility: ShaderStages::FRAGMENT,
                    ty: BindingType::Texture {
                        sample_type: TextureSampleType::Float { filterable: true },
                        view_dimension: TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                BindGroupLayoutEntry {
                    binding: 1,
                    visibility: ShaderStages::FRAGMENT,
                    ty: BindingType::Sampler(SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });

        let uniform_layout = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: Some("cian picture uniforms layout"),
            entries: &[BindGroupLayoutEntry {
                binding: 0,
                visibility: ShaderStages::FRAGMENT,
                ty: BindingType::Buffer {
                    ty: BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });

        // The colour-space flag is fixed for the life of the surface, so it is
        // written once here rather than every frame.
        let flags: [u32; 4] = [u32::from(surface_config.format.is_srgb()), 0, 0, 0];
        let uniform_buffer = device.create_buffer(&BufferDescriptor {
            label: Some("cian picture uniforms"),
            size: std::mem::size_of_val(&flags) as u64,
            usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
            mapped_at_creation: true,
        });
        uniform_buffer
            .slice(..)
            .get_mapped_range_mut()
            .copy_from_slice(bytemuck_bytes(&flags));
        uniform_buffer.unmap();

        let uniforms = device.create_bind_group(&BindGroupDescriptor {
            label: Some("cian picture uniforms bindings"),
            layout: &uniform_layout,
            entries: &[BindGroupEntry {
                binding: 0,
                resource: uniform_buffer.as_entire_binding(),
            }],
        });

        let sampler = device.create_sampler(&SamplerDescriptor {
            address_mode_u: AddressMode::ClampToEdge,
            address_mode_v: AddressMode::ClampToEdge,
            address_mode_w: AddressMode::ClampToEdge,
            // Linear, unlike the text: a photo scaled to fit a pane is being
            // resampled, and nearest-neighbour would make a mess of it.
            mag_filter: FilterMode::Linear,
            min_filter: FilterMode::Linear,
            mipmap_filter: MipmapFilterMode::Nearest,
            ..Default::default()
        });

        let shader = device.create_shader_module(include_wgsl!("pixels.wgsl"));
        let pipeline_layout = device.create_pipeline_layout(&PipelineLayoutDescriptor {
            label: Some("cian picture pipeline layout"),
            bind_group_layouts: &[&texture_layout, &uniform_layout],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&RenderPipelineDescriptor {
            label: Some("cian picture pipeline"),
            layout: Some(&pipeline_layout),
            vertex: VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: PipelineCompilationOptions::default(),
                buffers: &[VertexBufferLayout {
                    array_stride: VERTEX_BYTES,
                    step_mode: VertexStepMode::Vertex,
                    attributes: &vertex_attr_array![0 => Float32x2, 1 => Float32x2, 2 => Float32],
                }],
            },
            primitive: PrimitiveState {
                topology: PrimitiveTopology::TriangleList,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: MultisampleState::default(),
            fragment: Some(FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: PipelineCompilationOptions::default(),
                targets: &[Some(ColorTargetState {
                    format: surface_config.format,
                    // Straight alpha: an icon is mostly nothing, and the text
                    // under it has to show through the nothing.
                    blend: Some(BlendState::ALPHA_BLENDING),
                    write_mask: ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });

        let capacity = INITIAL_QUADS * 6;
        let vertices = device.create_buffer(&BufferDescriptor {
            label: Some("cian picture vertices"),
            size: capacity * VERTEX_BYTES,
            usage: BufferUsages::VERTEX | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        Self {
            text,
            device: device.clone(),
            pipeline,
            texture_layout,
            sampler,
            uniforms,
            vertices,
            capacity,
            cache: HashMap::new(),
            pending: Vec::new(),
            frame: Vec::new(),
            drawn: Vec::new(),
            surface: (surface_config.width as f32, surface_config.height as f32),
            keylog: std::env::var_os("CIAN_GUI_KEYLOG").is_some(),
        }
    }

    /// Ask for a frame when the pictures have changed but the text has not.
    ///
    /// Without this the backend skips the whole compositor — post processor and
    /// all — on any frame where no cell differs, which is exactly the common
    /// case here: the grid's labels settle immediately and the icons arrive
    /// over the next several frames. They were being handed over and never
    /// drawn. There is no clue on screen that this is what happened; the icons
    /// simply are not there.
    fn needs_update(&self) -> bool {
        self.frame != self.drawn || !self.pending.is_empty()
    }

    fn resize(
        &mut self,
        device: &Device,
        text_view: &TextureView,
        surface_config: &SurfaceConfiguration,
    ) {
        self.text.resize(device, text_view, surface_config);
        self.surface = (surface_config.width as f32, surface_config.height as f32);
    }

    fn process(
        &mut self,
        encoder: &mut CommandEncoder,
        queue: &Queue,
        text_view: &TextureView,
        surface_config: &SurfaceConfiguration,
        surface_view: &TextureView,
    ) {
        // Pictures that arrived since the last frame. Done before the text so a
        // brand-new picture is on the GPU by the time it is asked for.
        for upload in std::mem::take(&mut self.pending) {
            let bind = self.make_texture(queue, &upload);
            self.cache.insert(upload.id, bind);
        }

        self.text.process(encoder, queue, text_view, surface_config, surface_view);

        let draws: Vec<Draw> =
            self.frame.iter().copied().filter(|d| self.cache.contains_key(&d.id)).collect();
        self.drawn = self.frame.clone();
        if self.keylog {
            eprintln!(
                "layer {:p}: {} asked, {} drawn, {} textures",
                self as *const _,
                self.frame.len(),
                draws.len(),
                self.cache.len(),
            );
        }
        if draws.is_empty() {
            return;
        }

        let needed = draws.len() as u64 * 6;
        if needed > self.capacity {
            self.capacity = needed.next_power_of_two();
            self.vertices = self.device.create_buffer(&BufferDescriptor {
                label: Some("cian picture vertices"),
                size: self.capacity * VERTEX_BYTES,
                usage: BufferUsages::VERTEX | BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
        }

        let mut verts: Vec<Vertex> = Vec::with_capacity(needed as usize);
        for d in &draws {
            verts.extend_from_slice(&self.quad(d));
        }
        queue.write_buffer(&self.vertices, 0, bytemuck_bytes(&verts));

        // Load, not Clear: the text is already on the surface and the pictures
        // go over it.
        let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
            label: Some("cian picture pass"),
            color_attachments: &[Some(RenderPassColorAttachment {
                view: surface_view,
                resolve_target: None,
                ops: Operations { load: LoadOp::Load, store: StoreOp::Store },
                depth_slice: None,
            })],
            ..Default::default()
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(1, &self.uniforms, &[]);
        pass.set_vertex_buffer(0, self.vertices.slice(..));
        for (i, d) in draws.iter().enumerate() {
            let Some(bind) = self.cache.get(&d.id) else { continue };
            pass.set_bind_group(0, bind, &[]);
            let base = i as u32 * 6;
            pass.draw(base..base + 6, 0..1);
        }
    }
}

/// The bytes of a `#[repr(C)]`-shaped slice of plain data.
///
/// `bytemuck` is not a dependency here and pulling it in for two calls would be
/// the tail wagging the dog: both callers hand over slices of `f32`/`u32`
/// arrays, which have no padding and no invalid bit patterns.
fn bytemuck_bytes<T: Copy>(v: &[T]) -> &[u8] {
    // SAFETY: `T` is `[f32; 4]` or `[u32; 4]` at both call sites — plain old
    // data with no padding — and the slice is only read, for the length of the
    // borrow.
    unsafe { std::slice::from_raw_parts(v.as_ptr() as *const u8, std::mem::size_of_val(v)) }
}
