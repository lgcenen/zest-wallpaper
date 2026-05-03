use std::{collections::BTreeMap, fs, path::Path};

use glam::{EulerRot, Mat4, Quat, Vec2, Vec3, Vec4};

use crate::models::SceneAnimationLayer;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SceneMdlVertexEncoding {
    Standard,
    Extended,
    Compact,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SceneMdlContainerKind {
    InlineMesh,
    Puppet,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SceneMdlRawMesh {
    pub positions: Vec<Vec3>,
    pub uvs: Vec<Vec2>,
    pub blend_indices: Vec<[u32; 4]>,
    pub weights: Vec<[f32; 4]>,
    pub morph_indices: Option<Vec<i32>>,
    pub indices: Vec<u16>,
    pub encoding: SceneMdlVertexEncoding,
    pub mdl_flag: i32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SceneMdlSubmesh {
    pub table_index: usize,
    pub index_start: usize,
    pub index_count: usize,
    pub bone_index: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SceneMdlMaskBinding {
    pub mask_path: String,
    pub mask_bone: usize,
    pub target_submesh: usize,
    pub pass_type: usize,
    pub flag: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SceneMdlAttachment {
    pub name: String,
    pub bone_index: usize,
    pub matrix: [[f32; 4]; 4],
}

#[derive(Debug, Clone, PartialEq)]
pub struct SceneMdlBone {
    pub name: String,
    pub parent_index: i32,
    pub local_matrix: [[f32; 4]; 4],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SceneMdlPlayMode {
    Loop,
    Mirror,
    Single,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SceneMdlRotation {
    Euler(Vec3),
    Quaternion(Quat),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SceneMdlKeyframe {
    pub translation: Vec3,
    pub rotation: SceneMdlRotation,
    pub scale: Vec3,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SceneMdlAnimation {
    pub id: i32,
    pub name: String,
    pub mode: SceneMdlPlayMode,
    pub fps: f32,
    pub length: usize,
    pub frame_time: f64,
    pub max_time: f64,
    pub bone_tracks: Vec<Vec<SceneMdlKeyframe>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SceneMdlMorphTarget {
    pub id: u32,
    pub name: String,
    pub entries: Vec<[i16; 3]>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SceneMdlMorphSet {
    pub bounding: f32,
    pub targets: Vec<SceneMdlMorphTarget>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SceneMdlDocument {
    pub raw_mesh: Option<SceneMdlRawMesh>,
    pub submeshes: Vec<SceneMdlSubmesh>,
    pub mask_bindings: Vec<SceneMdlMaskBinding>,
    pub bones: Vec<SceneMdlBone>,
    pub attachments: Vec<SceneMdlAttachment>,
    pub animations: Vec<SceneMdlAnimation>,
    pub morphs: Option<SceneMdlMorphSet>,
    pub container_kind: SceneMdlContainerKind,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SceneMdlMeshFrame {
    pub positions: Vec<Vec3>,
    pub uvs: Vec<Vec2>,
    pub indices: Vec<u16>,
}

pub fn parse_scene_mdl_file(path: impl AsRef<Path>) -> Result<SceneMdlDocument, String> {
    let path = path.as_ref();
    let bytes =
        fs::read(path).map_err(|error| format!("unable to read {}: {error}", path.display()))?;
    parse_scene_mdl(&bytes)
}

pub fn parse_scene_mdl(bytes: &[u8]) -> Result<SceneMdlDocument, String> {
    let mut reader = SliceCursor::new(bytes);
    let raw_mesh = parse_mesh(&mut reader)?;
    let submeshes = parse_submeshes(
        bytes,
        raw_mesh
            .as_ref()
            .map(|mesh| mesh.indices.len())
            .unwrap_or_default(),
    );
    let mask_bindings = parse_mask_bindings(bytes);
    let bones = find_tag(bytes, b"MDLS")
        .map(|offset| parse_bones(bytes, offset))
        .transpose()?
        .unwrap_or_default();
    let attachments = find_tag(bytes, b"MDAT")
        .map(|offset| parse_attachments(bytes, offset))
        .transpose()?
        .unwrap_or_default();
    let animations = find_mdla(bytes)
        .map(|offset| parse_animations(bytes, offset))
        .transpose()?
        .unwrap_or_default();
    let morphs = find_tag(bytes, b"MDMP")
        .map(|offset| parse_morphs(bytes, offset))
        .transpose()?;

    let container_kind = if bones.is_empty()
        && attachments.is_empty()
        && animations.is_empty()
        && morphs.is_none()
        && mask_bindings.is_empty()
    {
        SceneMdlContainerKind::InlineMesh
    } else {
        SceneMdlContainerKind::Puppet
    };

    Ok(SceneMdlDocument {
        raw_mesh,
        submeshes,
        mask_bindings,
        bones,
        attachments,
        animations,
        morphs,
        container_kind,
    })
}

pub fn evaluate_scene_mdl_mesh(
    document: &SceneMdlDocument,
    layers: &[SceneAnimationLayer],
    elapsed_seconds: f64,
) -> Option<SceneMdlMeshFrame> {
    let raw = document.raw_mesh.as_ref()?;
    let local_bones = document
        .bones
        .iter()
        .map(|bone| mat4_from_cols(bone.local_matrix))
        .collect::<Vec<_>>();
    if local_bones.is_empty() {
        return Some(SceneMdlMeshFrame {
            positions: raw.positions.clone(),
            uvs: raw.uvs.clone(),
            indices: raw.indices.clone(),
        });
    }

    let bind_model = build_model_space_matrices(&local_bones, &document.bones);
    let inverse_bind = bind_model.iter().map(Mat4::inverse).collect::<Vec<_>>();
    let animated_locals = sample_animation_layers(document, layers, elapsed_seconds, &local_bones);
    let animated_model = build_model_space_matrices(&animated_locals, &document.bones);
    let positions = raw
        .positions
        .iter()
        .enumerate()
        .map(|(index, position)| {
            let indices = raw
                .blend_indices
                .get(index)
                .copied()
                .unwrap_or([0, 0, 0, 0]);
            let weights = raw
                .weights
                .get(index)
                .copied()
                .unwrap_or([1.0, 0.0, 0.0, 0.0]);
            skin_vertex(*position, indices, weights, &animated_model, &inverse_bind)
        })
        .collect::<Vec<_>>();

    Some(SceneMdlMeshFrame {
        positions,
        uvs: raw.uvs.clone(),
        indices: raw.indices.clone(),
    })
}

fn parse_mesh(reader: &mut SliceCursor<'_>) -> Result<Option<SceneMdlRawMesh>, String> {
    let magic = reader.read_cstring()?;
    if !magic.starts_with("MDLV") {
        return Err("MDL mesh section is missing MDLV header".to_string());
    }
    let mdl_flag = reader.read_i32()?;
    let _unk1 = reader.read_i32()?;
    let _unk2 = reader.read_i32()?;
    let _material_json = reader.read_cstring()?;
    let _padding = reader.read_i32()?;

    let header = read_mesh_header(reader)?;
    let vertex_count = header.vertex_size / header.vertex_stride;
    let mut positions = Vec::with_capacity(vertex_count);
    let mut uvs = Vec::with_capacity(vertex_count);
    let mut blend_indices = Vec::with_capacity(vertex_count);
    let mut weights = Vec::with_capacity(vertex_count);
    let mut morph_indices = (header.encoding == SceneMdlVertexEncoding::Compact)
        .then(Vec::new)
        .unwrap_or_default();

    match header.encoding {
        SceneMdlVertexEncoding::Standard => {
            for _ in 0..vertex_count {
                positions.push(Vec3::new(
                    reader.read_f32()?,
                    reader.read_f32()?,
                    reader.read_f32()?,
                ));
                blend_indices.push([
                    reader.read_u32()?,
                    reader.read_u32()?,
                    reader.read_u32()?,
                    reader.read_u32()?,
                ]);
                weights.push(normalize_weights([
                    reader.read_f32()?,
                    reader.read_f32()?,
                    reader.read_f32()?,
                    reader.read_f32()?,
                ]));
                uvs.push(Vec2::new(reader.read_f32()?, reader.read_f32()?));
            }
        }
        SceneMdlVertexEncoding::Extended => {
            for _ in 0..vertex_count {
                positions.push(Vec3::new(
                    reader.read_f32()?,
                    reader.read_f32()?,
                    reader.read_f32()?,
                ));
                for _ in 0..7 {
                    let _ = reader.read_u32()?;
                }
                blend_indices.push([
                    reader.read_u32()?,
                    reader.read_u32()?,
                    reader.read_u32()?,
                    reader.read_u32()?,
                ]);
                weights.push(normalize_weights([
                    reader.read_f32()?,
                    reader.read_f32()?,
                    reader.read_f32()?,
                    reader.read_f32()?,
                ]));
                uvs.push(Vec2::new(reader.read_f32()?, reader.read_f32()?));
            }
        }
        SceneMdlVertexEncoding::Compact => {
            if header.blocks_per_vertex == 3 {
                for _ in 0..vertex_count {
                    let block0 = read_compact_block(reader)?;
                    let block1 = read_compact_block(reader)?;
                    let block2 = read_compact_block(reader)?;
                    positions.push(Vec3::new(
                        f32::from_bits(block0[0]),
                        f32::from_bits(block0[1]),
                        f32::from_bits(block0[2]),
                    ));
                    morph_indices.push(f32::from_bits(block0[3]) as i32);
                    blend_indices.push([block2[0], block1[4], block1[5], block1[6]]);
                    weights.push(normalize_weights([
                        f32::from_bits(block1[1]),
                        f32::from_bits(block2[1]),
                        f32::from_bits(block2[2]),
                        f32::from_bits(block2[3]),
                    ]));
                    uvs.push(Vec2::new(
                        f32::from_bits(block2[5]),
                        f32::from_bits(block2[6]),
                    ));
                }
            } else {
                for _ in 0..vertex_count {
                    positions.push(Vec3::new(
                        reader.read_f32()?,
                        reader.read_f32()?,
                        reader.read_f32()?,
                    ));
                    morph_indices.push(reader.read_f32()? as i32);
                    blend_indices.push([reader.read_u32()?, 0, 0, 0]);
                    weights.push([1.0, 0.0, 0.0, 0.0]);
                    uvs.push(Vec2::new(reader.read_f32()?, reader.read_f32()?));
                }
            }
        }
    }

    let indices_size = reader.read_u32()? as usize;
    if indices_size == 0 {
        return Ok(Some(SceneMdlRawMesh {
            positions,
            uvs,
            blend_indices,
            weights,
            morph_indices: (!morph_indices.is_empty()).then_some(morph_indices),
            indices: Vec::new(),
            encoding: header.encoding,
            mdl_flag,
        }));
    }
    if indices_size % 2 != 0 {
        return Err("MDL index buffer size is invalid".to_string());
    }
    let index_count = indices_size / 2;
    let mut indices = Vec::with_capacity(index_count);
    for _ in 0..index_count {
        indices.push(reader.read_u16()?);
    }

    Ok(Some(SceneMdlRawMesh {
        positions,
        uvs,
        blend_indices,
        weights,
        morph_indices: (!morph_indices.is_empty()).then_some(morph_indices),
        indices,
        encoding: header.encoding,
        mdl_flag,
    }))
}

fn parse_submeshes(bytes: &[u8], total_index_count: usize) -> Vec<SceneMdlSubmesh> {
    let Some(anchor) = find_path_anchor(bytes, b"masks/") else {
        return Vec::new();
    };
    if anchor < 4 {
        return Vec::new();
    }
    let length = read_u32_le(bytes, anchor - 4).unwrap_or_default() as usize;
    if length == 0 || anchor < 4 + length || length % 16 != 0 {
        return Vec::new();
    }
    let table_start = anchor - 4 - length;
    let entry_count = length / 16;
    let mut result = Vec::with_capacity(entry_count);
    let mut running_end = 0_usize;
    for entry_index in 0..entry_count {
        let offset = table_start + entry_index * 16;
        let raw_count = read_u32_le(bytes, offset).unwrap_or_default() as usize;
        let bone_index = read_u32_le(bytes, offset + 4).unwrap_or_default() as usize;
        let raw_end = read_u32_le(bytes, offset + 12).unwrap_or_default() as usize;
        let end_index = if raw_end == 0 {
            running_end.saturating_add(raw_count)
        } else {
            raw_end
        };
        let index_start = end_index.saturating_sub(raw_count);
        running_end = end_index;
        result.push(SceneMdlSubmesh {
            table_index: entry_index + 1,
            index_start,
            index_count: raw_count,
            bone_index,
        });
    }
    if total_index_count > 0
        && result.iter().map(|entry| entry.index_count).sum::<usize>() != total_index_count
    {
        if result.len() > 1 {
            let trimmed = result.iter().skip(1).cloned().collect::<Vec<_>>();
            if trimmed.iter().map(|entry| entry.index_count).sum::<usize>() == total_index_count {
                return trimmed;
            }
        }
    }
    result
}

fn parse_mask_bindings(bytes: &[u8]) -> Vec<SceneMdlMaskBinding> {
    let mut results = Vec::new();
    let mut start = 0;
    while let Some(anchor) = find_bytes_from(bytes, b"masks/", start) {
        let Some(mask_path) = read_cstring_at(bytes, anchor) else {
            break;
        };
        let params_offset = anchor + mask_path.len() + 1;
        if params_offset + 20 <= bytes.len() {
            if let Some(binding) = parse_mask_binding_record(bytes, params_offset, &mask_path) {
                results.push(binding);
            }
        }
        start = anchor + mask_path.len().max(1);
    }
    results
}

fn parse_mask_binding_record(
    bytes: &[u8],
    offset: usize,
    mask_path: &str,
) -> Option<SceneMdlMaskBinding> {
    let big = [
        read_u32_be(bytes, offset)?,
        read_u32_be(bytes, offset + 4)?,
        read_u32_be(bytes, offset + 8)?,
        read_u32_be(bytes, offset + 12)?,
        read_u32_be(bytes, offset + 16)?,
    ];
    if big[0] == 1 {
        return Some(SceneMdlMaskBinding {
            mask_path: mask_path.to_string(),
            mask_bone: big[1] as usize,
            target_submesh: big[2] as usize,
            pass_type: big[3] as usize,
            flag: big[4] as usize,
        });
    }

    let little = [
        read_u32_le(bytes, offset)?,
        read_u32_le(bytes, offset + 4)?,
        read_u32_le(bytes, offset + 8)?,
        read_u32_le(bytes, offset + 12)?,
        read_u32_le(bytes, offset + 16)?,
    ];
    let scale = if little[0] == 256 { 256 } else { 1 };
    let normalized = little.map(|value| value / scale);
    (normalized[0] == 1).then(|| SceneMdlMaskBinding {
        mask_path: mask_path.to_string(),
        mask_bone: normalized[1] as usize,
        target_submesh: normalized[2] as usize,
        pass_type: normalized[3] as usize,
        flag: normalized[4] as usize,
    })
}

fn parse_bones(bytes: &[u8], offset: usize) -> Result<Vec<SceneMdlBone>, String> {
    let mut reader = SliceCursor::from_offset(bytes, offset);
    let tag = reader.read_cstring()?;
    if !tag.starts_with("MDLS") {
        return Err("MDLS section tag is invalid".to_string());
    }
    let _section_end = reader.read_u32()?;
    let bone_count = reader.read_u32()? as usize;
    let mut bones = Vec::with_capacity(bone_count);
    for _ in 0..bone_count {
        let name = reader.read_cstring()?;
        let _unk = reader.read_u32()?;
        let parent = reader.read_u32()? as i32;
        let matrix_bytes = reader.read_u32()? as usize;
        if matrix_bytes < 64 {
            return Err(format!("bone {name} has invalid transform payload"));
        }
        let local_matrix = read_mat4(&mut reader)?;
        if matrix_bytes > 64 {
            reader.skip(matrix_bytes - 64)?;
        }
        let _simulation = reader.read_cstring()?;
        bones.push(SceneMdlBone {
            name,
            parent_index: if parent == -1_i32 as u32 as i32 {
                -1
            } else {
                parent
            },
            local_matrix,
        });
    }
    if reader.remaining() >= 2 {
        let _ = reader.read_u16()?;
    }
    Ok(bones)
}

fn parse_attachments(bytes: &[u8], offset: usize) -> Result<Vec<SceneMdlAttachment>, String> {
    let mut reader = SliceCursor::from_offset(bytes, offset);
    let tag = reader.read_cstring()?;
    if !tag.starts_with("MDAT") {
        return Err("MDAT section tag is invalid".to_string());
    }
    let _unk = reader.read_u32()?;
    let count = reader.read_u16()? as usize;
    let mut attachments = Vec::with_capacity(count);
    for _ in 0..count {
        let bone_index = reader.read_u16()? as usize;
        let name = reader.read_cstring()?;
        let matrix = read_mat4(&mut reader)?;
        attachments.push(SceneMdlAttachment {
            name,
            bone_index,
            matrix,
        });
    }
    Ok(attachments)
}

fn parse_animations(bytes: &[u8], offset: usize) -> Result<Vec<SceneMdlAnimation>, String> {
    let mut reader = SliceCursor::from_offset(bytes, offset);
    let tag = reader.read_cstring()?;
    if !tag.starts_with("MDLA") {
        return Err("MDLA section tag is invalid".to_string());
    }
    let version = tag
        .strip_prefix("MDLA")
        .and_then(|raw| raw.parse::<u32>().ok())
        .unwrap_or(1);
    let next_offset = reader.read_u32()? as usize;
    let animation_count = reader.read_u32()? as usize;
    let mut cursor = reader.offset();
    let section_end = if next_offset > offset && next_offset <= bytes.len() {
        next_offset
    } else {
        bytes.len()
    };
    let mut animations = Vec::with_capacity(animation_count);
    for _ in 0..animation_count {
        let Some(anim_start) = find_next_animation(bytes, cursor, section_end) else {
            break;
        };
        let mut anim_reader = SliceCursor::from_offset(bytes, anim_start);
        let id = anim_reader.read_i32()?;
        let _unk = anim_reader.read_i32()?;
        let mut name = anim_reader.read_cstring()?;
        if name.is_empty() {
            name = anim_reader.read_cstring()?;
        }
        let mode = parse_play_mode(&anim_reader.read_cstring()?);
        let fps = anim_reader.read_f32()?;
        let length = anim_reader.read_i32()?.max(0) as usize;
        let _unk2 = anim_reader.read_i32()?;
        let track_count = anim_reader.read_u32()? as usize;
        let mut bone_tracks = Vec::with_capacity(track_count);
        for _ in 0..track_count {
            let _bone_id = anim_reader.read_i32()?;
            let byte_size = anim_reader.read_u32()? as usize;
            let use_quat = byte_size % 40 == 0 && byte_size % 36 != 0;
            let frame_count = if use_quat {
                byte_size / 40
            } else {
                byte_size / 36
            };
            let mut frames = Vec::with_capacity(frame_count);
            for _ in 0..frame_count {
                let translation = Vec3::new(
                    anim_reader.read_f32()?,
                    anim_reader.read_f32()?,
                    anim_reader.read_f32()?,
                );
                let rotation = if use_quat {
                    let qw = anim_reader.read_f32()?;
                    let qx = anim_reader.read_f32()?;
                    let qy = anim_reader.read_f32()?;
                    let qz = anim_reader.read_f32()?;
                    SceneMdlRotation::Quaternion(Quat::from_xyzw(qx, qy, qz, qw).normalize())
                } else {
                    SceneMdlRotation::Euler(Vec3::new(
                        anim_reader.read_f32()?,
                        anim_reader.read_f32()?,
                        anim_reader.read_f32()?,
                    ))
                };
                let scale = Vec3::new(
                    anim_reader.read_f32()?,
                    anim_reader.read_f32()?,
                    anim_reader.read_f32()?,
                );
                frames.push(SceneMdlKeyframe {
                    translation,
                    rotation,
                    scale,
                });
            }
            bone_tracks.push(frames);
        }
        match version {
            3 => {
                let _ = anim_reader.read_u8()?;
            }
            _ => {
                let extra_count = anim_reader.read_u32().unwrap_or_default() as usize;
                for _ in 0..extra_count {
                    let _ = anim_reader.read_f32()?;
                    let _ = anim_reader.read_cstring();
                }
            }
        }
        cursor = anim_reader.offset();
        let frame_time = if fps > 0.0 { 1.0 / fps as f64 } else { 0.0 };
        let max_time = if fps > 0.0 && length > 0 {
            length as f64 / fps as f64
        } else {
            0.0
        };
        animations.push(SceneMdlAnimation {
            id,
            name,
            mode,
            fps,
            length,
            frame_time,
            max_time,
            bone_tracks,
        });
    }
    Ok(animations)
}

fn parse_morphs(bytes: &[u8], offset: usize) -> Result<SceneMdlMorphSet, String> {
    let mut reader = SliceCursor::from_offset(bytes, offset);
    let tag = reader.read_cstring()?;
    if !tag.starts_with("MDMP") {
        return Err("MDMP section tag is invalid".to_string());
    }
    let _end_offset = reader.read_u32()?;
    let count = reader.read_u16()? as usize;
    let bounding = reader.read_f32()?;
    let mut targets = Vec::with_capacity(count);
    for _ in 0..count {
        let entry_count = reader.read_u32()? as usize;
        let id = reader.read_u32()?;
        let _padding = reader.read_u32()?;
        while reader.peek_u8() == Some(0) {
            let _ = reader.read_u8()?;
        }
        let name = reader.read_cstring()?;
        let byte_count = reader.read_u32()? as usize;
        let expected_count = byte_count / 6;
        let entry_total = entry_count.min(expected_count);
        let mut entries = Vec::with_capacity(entry_total);
        for _ in 0..entry_total {
            entries.push([reader.read_i16()?, reader.read_i16()?, reader.read_i16()?]);
        }
        let consumed = entry_total * 6;
        if byte_count > consumed {
            reader.skip(byte_count - consumed)?;
        }
        targets.push(SceneMdlMorphTarget { id, name, entries });
    }
    if let Ok(padding_size) = reader.read_u32() {
        if padding_size as usize <= reader.remaining() {
            let _ = reader.skip(padding_size as usize);
        }
    }
    Ok(SceneMdlMorphSet { bounding, targets })
}

#[derive(Clone, Copy)]
struct MeshHeader {
    encoding: SceneMdlVertexEncoding,
    vertex_size: usize,
    vertex_stride: usize,
    blocks_per_vertex: usize,
}

fn read_mesh_header(reader: &mut SliceCursor<'_>) -> Result<MeshHeader, String> {
    let standard_herald = 0x0180_0009_u32;
    let extended_herald = 0x0180_000F_u32;
    let compact_herald = 0x0181_000E_u32;
    let standard_stride = 52;
    let extended_stride = 80;
    let compact_stride = 28;

    let mut raw = reader.read_u32()?;
    let mut encoding = SceneMdlVertexEncoding::Standard;
    if raw == 0 {
        for _ in 0..1024 {
            raw = reader.read_u32()?;
            if raw == extended_herald {
                encoding = SceneMdlVertexEncoding::Extended;
                raw = reader.read_u32()?;
                break;
            }
            if raw == compact_herald {
                encoding = SceneMdlVertexEncoding::Compact;
                raw = reader.read_u32()?;
                break;
            }
        }
    } else if raw == standard_herald {
        encoding = SceneMdlVertexEncoding::Standard;
        raw = reader.read_u32()?;
    } else if raw == extended_herald {
        encoding = SceneMdlVertexEncoding::Extended;
        raw = reader.read_u32()?;
    } else if raw == compact_herald {
        encoding = SceneMdlVertexEncoding::Compact;
        raw = reader.read_u32()?;
    }

    let vertex_size = raw as usize;
    let (vertex_stride, blocks_per_vertex) = match encoding {
        SceneMdlVertexEncoding::Standard => (standard_stride, 1),
        SceneMdlVertexEncoding::Extended => (extended_stride, 1),
        SceneMdlVertexEncoding::Compact => {
            if vertex_size % (compact_stride * 3) == 0 {
                (compact_stride * 3, 3)
            } else {
                (compact_stride, 1)
            }
        }
    };
    if vertex_size == 0 || vertex_stride == 0 || vertex_size % vertex_stride != 0 {
        return Err("MDL vertex buffer layout is invalid".to_string());
    }
    Ok(MeshHeader {
        encoding,
        vertex_size,
        vertex_stride,
        blocks_per_vertex,
    })
}

fn sample_animation_layers(
    document: &SceneMdlDocument,
    layers: &[SceneAnimationLayer],
    elapsed_seconds: f64,
    bind_local_matrices: &[Mat4],
) -> Vec<Mat4> {
    let bind_pose = bind_local_matrices
        .iter()
        .map(|matrix| matrix.to_scale_rotation_translation())
        .collect::<Vec<_>>();
    if document.animations.is_empty() {
        return bind_local_matrices.to_vec();
    }
    let mut visible_layers = layers
        .iter()
        .filter(|layer| layer.visible)
        .collect::<Vec<_>>();
    if visible_layers.is_empty() {
        visible_layers = Vec::new();
    }

    let mut current = bind_pose
        .iter()
        .map(|(scale, rotation, translation)| PoseSample {
            translation: *translation,
            rotation: *rotation,
            scale: *scale,
        })
        .collect::<Vec<_>>();
    if let Some(base_layer) = visible_layers
        .iter()
        .find(|layer| !layer_is_additive(layer))
        .or_else(|| visible_layers.first())
    {
        let weight = layer_weight(base_layer);
        let sampled = sample_animation_pose(document, base_layer, elapsed_seconds, &bind_pose);
        for (current_pose, sampled_pose) in current.iter_mut().zip(sampled) {
            current_pose.translation = current_pose
                .translation
                .lerp(sampled_pose.translation, weight);
            current_pose.scale = current_pose.scale.lerp(sampled_pose.scale, weight);
            current_pose.rotation = current_pose.rotation.slerp(sampled_pose.rotation, weight);
        }
    }

    for layer in visible_layers
        .into_iter()
        .filter(|layer| layer_is_additive(layer))
    {
        let weight = layer_weight(layer);
        if weight <= 0.0 {
            continue;
        }
        let sampled = sample_animation_pose(document, layer, elapsed_seconds, &bind_pose);
        for ((current_pose, sampled_pose), bind_entry) in
            current.iter_mut().zip(sampled).zip(bind_pose.iter())
        {
            let (bind_scale, bind_rotation, bind_translation) = *bind_entry;
            current_pose.translation += (sampled_pose.translation - bind_translation) * weight;
            current_pose.scale += (sampled_pose.scale - bind_scale) * weight;
            let delta = bind_rotation.inverse() * sampled_pose.rotation;
            current_pose.rotation = current_pose.rotation * Quat::IDENTITY.slerp(delta, weight);
        }
    }

    current
        .into_iter()
        .map(|pose| {
            Mat4::from_scale_rotation_translation(pose.scale, pose.rotation, pose.translation)
        })
        .collect()
}

fn sample_animation_pose(
    document: &SceneMdlDocument,
    layer: &SceneAnimationLayer,
    elapsed_seconds: f64,
    bind_pose: &[(Vec3, Quat, Vec3)],
) -> Vec<PoseSample> {
    let animation = document
        .animations
        .iter()
        .find(|animation| animation.id == layer.id as i32 || animation.name == layer.animation)
        .or_else(|| document.animations.first());
    let Some(animation) = animation else {
        return bind_pose
            .iter()
            .map(|(scale, rotation, translation)| PoseSample {
                translation: *translation,
                rotation: *rotation,
                scale: *scale,
            })
            .collect();
    };
    bind_pose
        .iter()
        .enumerate()
        .map(
            |(bone_index, (bind_scale, bind_rotation, bind_translation))| {
                let Some(track) = animation.bone_tracks.get(bone_index) else {
                    return PoseSample {
                        translation: *bind_translation,
                        rotation: *bind_rotation,
                        scale: *bind_scale,
                    };
                };
                sample_animation_track(animation, track, elapsed_seconds * layer.rate)
            },
        )
        .collect()
}

fn sample_animation_track(
    animation: &SceneMdlAnimation,
    frames: &[SceneMdlKeyframe],
    raw_time: f64,
) -> PoseSample {
    if frames.is_empty() {
        return PoseSample::default();
    }
    if frames.len() == 1 || animation.frame_time <= 0.0 || animation.max_time <= 0.0 {
        return PoseSample::from_frame(frames[0]);
    }

    let mut time = raw_time;
    match animation.mode {
        SceneMdlPlayMode::Loop => {
            time = time.rem_euclid(animation.max_time);
        }
        SceneMdlPlayMode::Mirror => {
            let mirrored = (animation.max_time * 2.0).max(animation.frame_time);
            let normalized = time.rem_euclid(mirrored);
            time = if normalized > animation.max_time {
                mirrored - normalized
            } else {
                normalized
            };
        }
        SceneMdlPlayMode::Single => {
            time = time.clamp(0.0, animation.max_time);
        }
    }
    let frame_space = (time / animation.frame_time).clamp(0.0, (frames.len() - 1) as f64);
    let left = frame_space.floor() as usize;
    let right = (left + 1).min(frames.len() - 1);
    let t = (frame_space - left as f64) as f32;
    let from = PoseSample::from_frame(frames[left]);
    let to = PoseSample::from_frame(frames[right]);
    PoseSample {
        translation: from.translation.lerp(to.translation, t),
        rotation: from.rotation.slerp(to.rotation, t),
        scale: from.scale.lerp(to.scale, t),
    }
}

fn build_model_space_matrices(local_matrices: &[Mat4], bones: &[SceneMdlBone]) -> Vec<Mat4> {
    let mut cache = BTreeMap::new();
    (0..local_matrices.len())
        .map(|index| resolve_model_matrix(index, local_matrices, bones, &mut cache))
        .collect()
}

fn resolve_model_matrix(
    index: usize,
    local_matrices: &[Mat4],
    bones: &[SceneMdlBone],
    cache: &mut BTreeMap<usize, Mat4>,
) -> Mat4 {
    if let Some(existing) = cache.get(&index) {
        return *existing;
    }
    let local = local_matrices.get(index).copied().unwrap_or(Mat4::IDENTITY);
    let parent_index = bones.get(index).map(|bone| bone.parent_index).unwrap_or(-1);
    let model = if parent_index >= 0 {
        resolve_model_matrix(parent_index as usize, local_matrices, bones, cache) * local
    } else {
        local
    };
    cache.insert(index, model);
    model
}

fn skin_vertex(
    position: Vec3,
    indices: [u32; 4],
    weights: [f32; 4],
    animated_model: &[Mat4],
    inverse_bind: &[Mat4],
) -> Vec3 {
    let mut output = Vec3::ZERO;
    let mut total = 0.0_f32;
    let position4 = position.extend(1.0);
    for (slot, weight) in weights.iter().copied().enumerate() {
        if weight <= 0.0 {
            continue;
        }
        total += weight;
        let bone_index = indices[slot] as usize;
        let transform = animated_model
            .get(bone_index)
            .copied()
            .unwrap_or(Mat4::IDENTITY)
            * inverse_bind
                .get(bone_index)
                .copied()
                .unwrap_or(Mat4::IDENTITY);
        output += (transform * position4).truncate() * weight;
    }
    if total <= 0.0 {
        position
    } else {
        output
    }
}

fn find_mdla(bytes: &[u8]) -> Option<usize> {
    [
        b"MDLA0006",
        b"MDLA0005",
        b"MDLA0004",
        b"MDLA0003",
        b"MDLA0002",
        b"MDLA0001",
    ]
    .iter()
    .find_map(|tag| find_bytes_from(bytes, tag.as_slice(), 0))
    .or_else(|| find_tag(bytes, b"MDLA"))
}

fn find_next_animation(bytes: &[u8], start: usize, limit: usize) -> Option<usize> {
    let mut cursor = start;
    while cursor + 4 <= limit {
        let id = read_i32_le(bytes, cursor).unwrap_or_default();
        if id > 0 && id < 1_000_000 {
            let mut probe = SliceCursor::from_offset(bytes, cursor);
            let _ = probe.read_i32().ok()?;
            let _ = probe.read_i32().ok()?;
            let name = probe.read_cstring().ok()?;
            if !name.is_empty() {
                let mode = probe.read_cstring().ok()?;
                if matches!(mode.as_str(), "loop" | "mirror" | "single") {
                    let fps = probe.read_f32().ok()?;
                    let length = probe.read_i32().ok()?;
                    let _ = probe.read_i32().ok()?;
                    let track_count = probe.read_u32().ok()?;
                    if (0.0..240.0).contains(&fps) && length > 0 && (1..512).contains(&track_count)
                    {
                        return Some(cursor);
                    }
                }
            }
        }
        cursor += 1;
    }
    None
}

fn find_tag(bytes: &[u8], prefix: &[u8]) -> Option<usize> {
    find_bytes_from(bytes, prefix, 0)
}

fn find_path_anchor(bytes: &[u8], needle: &[u8]) -> Option<usize> {
    find_bytes_from(bytes, needle, 0)
}

fn find_bytes_from(bytes: &[u8], needle: &[u8], start: usize) -> Option<usize> {
    if needle.is_empty() || start >= bytes.len() {
        return None;
    }
    bytes[start..]
        .windows(needle.len())
        .position(|window| window == needle)
        .map(|offset| start + offset)
}

fn read_cstring_at(bytes: &[u8], offset: usize) -> Option<String> {
    let tail = bytes.get(offset..)?;
    let end = tail
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(tail.len());
    String::from_utf8(tail[..end].to_vec()).ok()
}

fn read_u32_le(bytes: &[u8], offset: usize) -> Option<u32> {
    let chunk = bytes.get(offset..offset + 4)?;
    Some(u32::from_le_bytes(chunk.try_into().ok()?))
}

fn read_u32_be(bytes: &[u8], offset: usize) -> Option<u32> {
    let chunk = bytes.get(offset..offset + 4)?;
    Some(u32::from_be_bytes(chunk.try_into().ok()?))
}

fn read_i32_le(bytes: &[u8], offset: usize) -> Option<i32> {
    let chunk = bytes.get(offset..offset + 4)?;
    Some(i32::from_le_bytes(chunk.try_into().ok()?))
}

fn read_compact_block(reader: &mut SliceCursor<'_>) -> Result<[u32; 7], String> {
    Ok([
        reader.read_u32()?,
        reader.read_u32()?,
        reader.read_u32()?,
        reader.read_u32()?,
        reader.read_u32()?,
        reader.read_u32()?,
        reader.read_u32()?,
    ])
}

fn read_mat4(reader: &mut SliceCursor<'_>) -> Result<[[f32; 4]; 4], String> {
    Ok([
        [
            reader.read_f32()?,
            reader.read_f32()?,
            reader.read_f32()?,
            reader.read_f32()?,
        ],
        [
            reader.read_f32()?,
            reader.read_f32()?,
            reader.read_f32()?,
            reader.read_f32()?,
        ],
        [
            reader.read_f32()?,
            reader.read_f32()?,
            reader.read_f32()?,
            reader.read_f32()?,
        ],
        [
            reader.read_f32()?,
            reader.read_f32()?,
            reader.read_f32()?,
            reader.read_f32()?,
        ],
    ])
}

fn mat4_from_cols(columns: [[f32; 4]; 4]) -> Mat4 {
    Mat4::from_cols(
        Vec4::from(columns[0]),
        Vec4::from(columns[1]),
        Vec4::from(columns[2]),
        Vec4::from(columns[3]),
    )
}

fn normalize_weights(weights: [f32; 4]) -> [f32; 4] {
    let sanitized = weights.map(|weight| {
        if weight.is_finite() {
            weight.max(0.0)
        } else {
            0.0
        }
    });
    let total = sanitized.iter().sum::<f32>();
    if total <= 0.0 {
        [1.0, 0.0, 0.0, 0.0]
    } else {
        sanitized.map(|weight| weight / total)
    }
}

fn parse_play_mode(raw: &str) -> SceneMdlPlayMode {
    match raw {
        "mirror" => SceneMdlPlayMode::Mirror,
        "single" => SceneMdlPlayMode::Single,
        _ => SceneMdlPlayMode::Loop,
    }
}

fn layer_is_additive(layer: &SceneAnimationLayer) -> bool {
    layer.blend.to_ascii_lowercase().contains("add")
}

fn layer_weight(layer: &SceneAnimationLayer) -> f32 {
    layer
        .blend
        .parse::<f32>()
        .ok()
        .unwrap_or(1.0)
        .clamp(0.0, 1.0)
}

#[derive(Clone, Copy, Debug, Default)]
struct PoseSample {
    translation: Vec3,
    rotation: Quat,
    scale: Vec3,
}

impl PoseSample {
    fn from_frame(frame: SceneMdlKeyframe) -> Self {
        Self {
            translation: frame.translation,
            rotation: match frame.rotation {
                SceneMdlRotation::Euler(angles) => {
                    Quat::from_euler(EulerRot::XYZ, angles.x, angles.y, angles.z)
                }
                SceneMdlRotation::Quaternion(quat) => quat,
            },
            scale: frame.scale,
        }
    }
}

struct SliceCursor<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> SliceCursor<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    fn from_offset(bytes: &'a [u8], offset: usize) -> Self {
        Self { bytes, offset }
    }

    fn offset(&self) -> usize {
        self.offset
    }

    fn remaining(&self) -> usize {
        self.bytes.len().saturating_sub(self.offset)
    }

    fn read_u8(&mut self) -> Result<u8, String> {
        let byte = *self
            .bytes
            .get(self.offset)
            .ok_or_else(|| "unexpected end of MDL payload".to_string())?;
        self.offset += 1;
        Ok(byte)
    }

    fn peek_u8(&self) -> Option<u8> {
        self.bytes.get(self.offset).copied()
    }

    fn read_u16(&mut self) -> Result<u16, String> {
        let chunk = self
            .bytes
            .get(self.offset..self.offset + 2)
            .ok_or_else(|| "unexpected end of MDL payload".to_string())?;
        self.offset += 2;
        Ok(u16::from_le_bytes(chunk.try_into().expect("slice len")))
    }

    fn read_i16(&mut self) -> Result<i16, String> {
        let chunk = self
            .bytes
            .get(self.offset..self.offset + 2)
            .ok_or_else(|| "unexpected end of MDL payload".to_string())?;
        self.offset += 2;
        Ok(i16::from_le_bytes(chunk.try_into().expect("slice len")))
    }

    fn read_u32(&mut self) -> Result<u32, String> {
        let chunk = self
            .bytes
            .get(self.offset..self.offset + 4)
            .ok_or_else(|| "unexpected end of MDL payload".to_string())?;
        self.offset += 4;
        Ok(u32::from_le_bytes(chunk.try_into().expect("slice len")))
    }

    fn read_i32(&mut self) -> Result<i32, String> {
        let chunk = self
            .bytes
            .get(self.offset..self.offset + 4)
            .ok_or_else(|| "unexpected end of MDL payload".to_string())?;
        self.offset += 4;
        Ok(i32::from_le_bytes(chunk.try_into().expect("slice len")))
    }

    fn read_f32(&mut self) -> Result<f32, String> {
        Ok(f32::from_bits(self.read_u32()?))
    }

    fn read_cstring(&mut self) -> Result<String, String> {
        let tail = self
            .bytes
            .get(self.offset..)
            .ok_or_else(|| "unexpected end of MDL payload".to_string())?;
        let end = tail
            .iter()
            .position(|byte| *byte == 0)
            .unwrap_or(tail.len());
        let string = String::from_utf8(tail[..end].to_vec())
            .map_err(|_| "MDL string is not valid UTF-8".to_string())?;
        self.offset += end.saturating_add(1);
        Ok(string)
    }

    fn skip(&mut self, count: usize) -> Result<(), String> {
        if self.remaining() < count {
            return Err("unexpected end of MDL payload".to_string());
        }
        self.offset += count;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{
        evaluate_scene_mdl_mesh, parse_scene_mdl, SceneMdlPlayMode, SceneMdlVertexEncoding,
    };
    use crate::models::SceneAnimationLayer;

    fn push_cstring(bytes: &mut Vec<u8>, value: &str) {
        bytes.extend_from_slice(value.as_bytes());
        bytes.push(0);
    }

    fn push_u16(bytes: &mut Vec<u8>, value: u16) {
        bytes.extend_from_slice(&value.to_le_bytes());
    }

    fn push_i16(bytes: &mut Vec<u8>, value: i16) {
        bytes.extend_from_slice(&value.to_le_bytes());
    }

    fn push_u32(bytes: &mut Vec<u8>, value: u32) {
        bytes.extend_from_slice(&value.to_le_bytes());
    }

    fn push_i32(bytes: &mut Vec<u8>, value: i32) {
        bytes.extend_from_slice(&value.to_le_bytes());
    }

    fn push_be_u32(bytes: &mut Vec<u8>, value: u32) {
        bytes.extend_from_slice(&value.to_be_bytes());
    }

    fn push_f32(bytes: &mut Vec<u8>, value: f32) {
        bytes.extend_from_slice(&value.to_bits().to_le_bytes());
    }

    fn push_identity_mat4(bytes: &mut Vec<u8>) {
        for column in 0..4 {
            for row in 0..4 {
                push_f32(bytes, if column == row { 1.0 } else { 0.0 });
            }
        }
    }

    fn synthetic_standard_fixture() -> Vec<u8> {
        let mut bytes = Vec::new();
        push_cstring(&mut bytes, "MDLV0023");
        push_i32(&mut bytes, 1);
        push_i32(&mut bytes, 1);
        push_i32(&mut bytes, 1);
        push_cstring(&mut bytes, "materials/hero.material");
        push_i32(&mut bytes, 0);
        push_u32(&mut bytes, 0x0180_0009);
        push_u32(&mut bytes, 52 * 3);

        let vertices = [
            ([0.0, 0.0, 0.0], [1.0, 0.0, 0.0, 0.0], [0.0, 0.0]),
            ([64.0, 0.0, 0.0], [1.0, 0.0, 0.0, 0.0], [1.0, 0.0]),
            ([0.0, 64.0, 0.0], [1.0, 0.0, 0.0, 0.0], [0.0, 1.0]),
        ];
        for (position, weight, uv) in vertices {
            for value in position {
                push_f32(&mut bytes, value);
            }
            push_u32(&mut bytes, 0);
            push_u32(&mut bytes, 0);
            push_u32(&mut bytes, 0);
            push_u32(&mut bytes, 0);
            for value in weight {
                push_f32(&mut bytes, value);
            }
            push_f32(&mut bytes, uv[0]);
            push_f32(&mut bytes, uv[1]);
        }
        push_u32(&mut bytes, 6);
        push_u16(&mut bytes, 0);
        push_u16(&mut bytes, 1);
        push_u16(&mut bytes, 2);

        push_u32(&mut bytes, 3);
        push_u32(&mut bytes, 0);
        push_u32(&mut bytes, 0);
        push_u32(&mut bytes, 3);
        push_u32(&mut bytes, 16);
        push_cstring(&mut bytes, "masks/eye_mask");
        push_be_u32(&mut bytes, 1);
        push_be_u32(&mut bytes, 0);
        push_be_u32(&mut bytes, 1);
        push_be_u32(&mut bytes, 2);
        push_be_u32(&mut bytes, 1);

        push_cstring(&mut bytes, "MDLS0004");
        push_u32(&mut bytes, 0);
        push_u32(&mut bytes, 1);
        push_cstring(&mut bytes, "root");
        push_u32(&mut bytes, 0);
        push_u32(&mut bytes, u32::MAX);
        push_u32(&mut bytes, 64);
        push_identity_mat4(&mut bytes);
        push_cstring(&mut bytes, "{}");
        push_u16(&mut bytes, 0);

        push_cstring(&mut bytes, "MDAT0001");
        push_u32(&mut bytes, 0);
        push_u16(&mut bytes, 1);
        push_u16(&mut bytes, 0);
        push_cstring(&mut bytes, "socket_hat");
        push_identity_mat4(&mut bytes);

        push_cstring(&mut bytes, "MDLA0001");
        push_u32(&mut bytes, 0);
        push_u32(&mut bytes, 1);
        push_i32(&mut bytes, 7);
        push_i32(&mut bytes, 0);
        push_cstring(&mut bytes, "idle");
        push_cstring(&mut bytes, "loop");
        push_f32(&mut bytes, 24.0);
        push_i32(&mut bytes, 2);
        push_i32(&mut bytes, 0);
        push_u32(&mut bytes, 1);
        push_i32(&mut bytes, 0);
        push_u32(&mut bytes, 72);
        push_f32(&mut bytes, 0.0);
        push_f32(&mut bytes, 0.0);
        push_f32(&mut bytes, 0.0);
        push_f32(&mut bytes, 0.0);
        push_f32(&mut bytes, 0.0);
        push_f32(&mut bytes, 0.0);
        push_f32(&mut bytes, 1.0);
        push_f32(&mut bytes, 1.0);
        push_f32(&mut bytes, 1.0);
        push_f32(&mut bytes, 10.0);
        push_f32(&mut bytes, 0.0);
        push_f32(&mut bytes, 0.0);
        push_f32(&mut bytes, 0.0);
        push_f32(&mut bytes, 0.0);
        push_f32(&mut bytes, 0.0);
        push_f32(&mut bytes, 1.0);
        push_f32(&mut bytes, 1.0);
        push_f32(&mut bytes, 1.0);
        push_u32(&mut bytes, 0);

        push_cstring(&mut bytes, "MDMP0001");
        push_u32(&mut bytes, 0);
        push_u16(&mut bytes, 1);
        push_f32(&mut bytes, 2.0);
        push_u32(&mut bytes, 1);
        push_u32(&mut bytes, 513);
        push_u32(&mut bytes, 0);
        push_cstring(&mut bytes, "Smile");
        push_u32(&mut bytes, 6);
        push_i16(&mut bytes, 1200);
        push_i16(&mut bytes, -1200);
        push_i16(&mut bytes, 0);
        push_u32(&mut bytes, 0);

        bytes
    }

    fn synthetic_compact_fixture() -> Vec<u8> {
        let mut bytes = Vec::new();
        push_cstring(&mut bytes, "MDLV0021");
        push_i32(&mut bytes, 1);
        push_i32(&mut bytes, 1);
        push_i32(&mut bytes, 1);
        push_cstring(&mut bytes, "materials/compact.material");
        push_i32(&mut bytes, 0);
        push_u32(&mut bytes, 0x0181_000E);
        push_u32(&mut bytes, 28);
        push_f32(&mut bytes, 4.0);
        push_f32(&mut bytes, 5.0);
        push_f32(&mut bytes, 0.0);
        push_f32(&mut bytes, 3.0);
        push_u32(&mut bytes, 2);
        push_f32(&mut bytes, 0.25);
        push_f32(&mut bytes, 0.75);
        push_u32(&mut bytes, 0);
        bytes
    }

    #[test]
    fn parses_standard_mdl_sections_and_runtime_pose() {
        let document = parse_scene_mdl(&synthetic_standard_fixture()).expect("parse mdl");

        let raw = document.raw_mesh.as_ref().expect("raw mesh");
        assert_eq!(raw.encoding, SceneMdlVertexEncoding::Standard);
        assert_eq!(raw.mdl_flag, 1);
        assert_eq!(raw.positions.len(), 3);
        assert_eq!(document.submeshes.len(), 1);
        assert_eq!(document.mask_bindings.len(), 1);
        assert_eq!(document.attachments.len(), 1);
        assert_eq!(document.animations.len(), 1);
        assert_eq!(document.animations[0].mode, SceneMdlPlayMode::Loop);
        assert_eq!(document.morphs.as_ref().expect("morphs").targets.len(), 1);
        assert_eq!(
            document.container_kind,
            super::SceneMdlContainerKind::Puppet
        );

        let frame = evaluate_scene_mdl_mesh(
            &document,
            &[SceneAnimationLayer {
                id: 7,
                rate: 1.0,
                visible: true,
                visibility_binding: None,
                blend: "1".to_string(),
                animation: "idle".to_string(),
            }],
            1.0 / 24.0,
        )
        .expect("animated frame");

        assert_eq!(frame.indices, vec![0, 1, 2]);
        assert!(frame.positions[0].x > 0.0);
        assert!(frame.positions[1].x > frame.positions[0].x);
    }

    #[test]
    fn parses_compact_single_bone_vertices() {
        let document = parse_scene_mdl(&synthetic_compact_fixture()).expect("parse compact mdl");
        let raw = document.raw_mesh.as_ref().expect("raw mesh");
        assert_eq!(raw.encoding, SceneMdlVertexEncoding::Compact);
        assert_eq!(raw.mdl_flag, 1);
        assert_eq!(raw.positions.len(), 1);
        assert_eq!(raw.blend_indices[0][0], 2);
        assert_eq!(raw.morph_indices.as_ref().expect("morph indices")[0], 3);
        assert_eq!(raw.indices.len(), 0);
        assert_eq!(
            document.container_kind,
            super::SceneMdlContainerKind::InlineMesh
        );
    }

    fn synthetic_inline_mesh_flag_9_fixture() -> Vec<u8> {
        let mut bytes = Vec::new();
        push_cstring(&mut bytes, "MDLV0014");
        push_i32(&mut bytes, 9);
        push_i32(&mut bytes, 1);
        push_i32(&mut bytes, 1);
        push_cstring(&mut bytes, "materials/inline.material");
        push_i32(&mut bytes, 0);
        push_u32(&mut bytes, 0x0180_0009);
        push_u32(&mut bytes, 52 * 2);
        for (position, uv) in [
            ([0.0, 0.0, 0.0], [0.0, 0.0]),
            ([32.0, 32.0, 0.0], [1.0, 1.0]),
        ] {
            for value in position {
                push_f32(&mut bytes, value);
            }
            push_u32(&mut bytes, 0);
            push_u32(&mut bytes, 0);
            push_u32(&mut bytes, 0);
            push_u32(&mut bytes, 0);
            push_f32(&mut bytes, 1.0);
            push_f32(&mut bytes, 0.0);
            push_f32(&mut bytes, 0.0);
            push_f32(&mut bytes, 0.0);
            push_f32(&mut bytes, uv[0]);
            push_f32(&mut bytes, uv[1]);
        }
        push_u32(&mut bytes, 6);
        push_u16(&mut bytes, 0);
        push_u16(&mut bytes, 1);
        push_u16(&mut bytes, 0);
        bytes
    }

    #[test]
    fn parses_inline_mesh_with_flag_9_and_payload() {
        let document =
            parse_scene_mdl(&synthetic_inline_mesh_flag_9_fixture()).expect("parse inline mdl");
        let raw = document.raw_mesh.as_ref().expect("raw mesh");
        assert_eq!(raw.mdl_flag, 9, "flag 9 must not prevent mesh parsing");
        assert_eq!(raw.encoding, super::SceneMdlVertexEncoding::Standard);
        assert_eq!(raw.positions.len(), 2);
        assert_eq!(raw.indices, vec![0, 1, 0]);
        assert_eq!(
            document.container_kind,
            super::SceneMdlContainerKind::InlineMesh
        );
    }

    #[test]
    fn inline_mesh_without_bones_returns_static_frame() {
        let document =
            parse_scene_mdl(&synthetic_inline_mesh_flag_9_fixture()).expect("parse inline mdl");
        let frame = evaluate_scene_mdl_mesh(&document, &[], 0.0).expect("static frame");
        assert_eq!(frame.positions.len(), 2);
        assert!((frame.positions[0].x - 0.0).abs() < 0.001);
        assert!((frame.positions[1].x - 32.0).abs() < 0.001);
    }

    fn synthetic_static_inline_fixture() -> Vec<u8> {
        let mut bytes = Vec::new();
        push_cstring(&mut bytes, "MDLV0004");
        push_i32(&mut bytes, 11);
        push_i32(&mut bytes, 1);
        push_i32(&mut bytes, 1);
        push_cstring(&mut bytes, "materials/static.material");
        push_i32(&mut bytes, 0);
        push_u32(&mut bytes, 0x0180_0009);
        push_u32(&mut bytes, 52 * 3);
        let vertices = [
            ([0.0, 0.0, 0.0], [1.0, 0.0, 0.0, 0.0], [0.0, 0.0]),
            ([16.0, 0.0, 0.0], [1.0, 0.0, 0.0, 0.0], [1.0, 0.0]),
            ([0.0, 16.0, 0.0], [1.0, 0.0, 0.0, 0.0], [0.0, 1.0]),
        ];
        for (position, weight, uv) in vertices {
            for value in position {
                push_f32(&mut bytes, value);
            }
            push_u32(&mut bytes, 0);
            push_u32(&mut bytes, 0);
            push_u32(&mut bytes, 0);
            push_u32(&mut bytes, 0);
            for value in weight {
                push_f32(&mut bytes, value);
            }
            push_f32(&mut bytes, uv[0]);
            push_f32(&mut bytes, uv[1]);
        }
        push_u32(&mut bytes, 6);
        push_u16(&mut bytes, 0);
        push_u16(&mut bytes, 1);
        push_u16(&mut bytes, 2);
        bytes
    }

    #[test]
    fn classifies_container_as_inline_mesh_when_no_optional_sections() {
        let document =
            parse_scene_mdl(&synthetic_static_inline_fixture()).expect("parse static mdl");
        assert!(document.raw_mesh.is_some());
        assert!(document.bones.is_empty());
        assert!(document.animations.is_empty());
        assert!(document.attachments.is_empty());
        assert!(document.morphs.is_none());
        assert!(document.mask_bindings.is_empty());
        assert_eq!(
            document.container_kind,
            super::SceneMdlContainerKind::InlineMesh
        );
    }

    #[test]
    fn flag_9_with_optional_sections_is_classified_as_puppet() {
        let mut bytes = synthetic_inline_mesh_flag_9_fixture();
        push_cstring(&mut bytes, "MDLS0004");
        push_u32(&mut bytes, 0);
        push_u32(&mut bytes, 1);
        push_cstring(&mut bytes, "root");
        push_u32(&mut bytes, 0);
        push_u32(&mut bytes, u32::MAX);
        push_u32(&mut bytes, 64);
        push_identity_mat4(&mut bytes);
        push_cstring(&mut bytes, "{}");
        push_u16(&mut bytes, 0);

        let document = parse_scene_mdl(&bytes).expect("parse flag 9 puppet");
        let raw = document.raw_mesh.as_ref().expect("raw mesh");
        assert_eq!(raw.mdl_flag, 9);
        assert_eq!(document.bones.len(), 1);
        assert_eq!(
            document.container_kind,
            super::SceneMdlContainerKind::Puppet
        );
    }
}
