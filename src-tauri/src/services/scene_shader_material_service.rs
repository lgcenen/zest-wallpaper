use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

use serde_json::Value;

use crate::services::{
    scene_render_planner_service::SceneRenderBlendMode,
    scene_resource_service::{
        SceneResourceLookup, SceneResourceResolver, SceneResourceRootKind, SceneShaderSourceKind,
        SceneShaderSourceLookup,
    },
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SceneShaderProgramKind {
    Sprite,
    Model,
    MaskAlpha,
    MaskApply,
    Copy,
    EffectCompat(SceneCompatEffectKind),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum SceneCompatEffectKind {
    Pulse,
    Shake,
    WaterRipple,
    WaterWaves,
    Tint,
    Scroll,
    LightShafts,
    FoliageSway,
    Circle,
    Opacity,
    Transform,
    Skew,
    Perspective,
    Spin,
    Swing,
    Twirl,
    ChromaticAberration,
    ColorKey,
    FishEye,
    EdgeDetection,
    Iris,
    CloudMotion,
    Clouds,
    WaterFlow,
    Nitro,
    Blend,
    Reflection,
    Shimmer,
    FilmGrain,
    Vhs,
    BlendGradient,
    WaterCaustics,
    XRay,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScenePhase10bBindingSemantic {
    PreviousInput,
    NoiseTexture,
    FlowMap,
    TimeOffset,
    OpacityMask,
    NormalMap,
    GradientTexture,
    SpriteTexture,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScenePhase10bUvSpace {
    PrimaryInput,
    AuxTexture,
    MaskTexture,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScenePhase10bTextureSlotContract {
    pub slot: usize,
    pub semantic: ScenePhase10bBindingSemantic,
    pub uv_space: ScenePhase10bUvSpace,
    pub required: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScenePhase10bEffectContract {
    pub kind: SceneCompatEffectKind,
    pub family: &'static str,
    pub required_texture_slots: &'static [usize],
    pub supported_texture_slots: &'static [usize],
    pub supported_combo_defaults: &'static [(&'static str, i32)],
    pub supported_uniforms: &'static [&'static str],
    pub runtime_binding_layout: &'static [ScenePhase10bTextureSlotContract],
}

const PULSE_TEXTURE_SLOTS: &[ScenePhase10bTextureSlotContract] = &[
    ScenePhase10bTextureSlotContract {
        slot: 0,
        semantic: ScenePhase10bBindingSemantic::PreviousInput,
        uv_space: ScenePhase10bUvSpace::PrimaryInput,
        required: true,
    },
    ScenePhase10bTextureSlotContract {
        slot: 1,
        semantic: ScenePhase10bBindingSemantic::NoiseTexture,
        uv_space: ScenePhase10bUvSpace::AuxTexture,
        required: false,
    },
    ScenePhase10bTextureSlotContract {
        slot: 2,
        semantic: ScenePhase10bBindingSemantic::OpacityMask,
        uv_space: ScenePhase10bUvSpace::MaskTexture,
        required: false,
    },
];

const SHAKE_TEXTURE_SLOTS: &[ScenePhase10bTextureSlotContract] = &[
    ScenePhase10bTextureSlotContract {
        slot: 0,
        semantic: ScenePhase10bBindingSemantic::PreviousInput,
        uv_space: ScenePhase10bUvSpace::PrimaryInput,
        required: true,
    },
    ScenePhase10bTextureSlotContract {
        slot: 1,
        semantic: ScenePhase10bBindingSemantic::FlowMap,
        uv_space: ScenePhase10bUvSpace::AuxTexture,
        required: true,
    },
    ScenePhase10bTextureSlotContract {
        slot: 2,
        semantic: ScenePhase10bBindingSemantic::TimeOffset,
        uv_space: ScenePhase10bUvSpace::AuxTexture,
        required: false,
    },
    ScenePhase10bTextureSlotContract {
        slot: 3,
        semantic: ScenePhase10bBindingSemantic::OpacityMask,
        uv_space: ScenePhase10bUvSpace::MaskTexture,
        required: false,
    },
];

const WATERRIPPLE_TEXTURE_SLOTS: &[ScenePhase10bTextureSlotContract] = &[
    ScenePhase10bTextureSlotContract {
        slot: 0,
        semantic: ScenePhase10bBindingSemantic::PreviousInput,
        uv_space: ScenePhase10bUvSpace::PrimaryInput,
        required: true,
    },
    ScenePhase10bTextureSlotContract {
        slot: 1,
        semantic: ScenePhase10bBindingSemantic::OpacityMask,
        uv_space: ScenePhase10bUvSpace::MaskTexture,
        required: false,
    },
    ScenePhase10bTextureSlotContract {
        slot: 2,
        semantic: ScenePhase10bBindingSemantic::NormalMap,
        uv_space: ScenePhase10bUvSpace::AuxTexture,
        required: true,
    },
];

const WATERWAVES_TEXTURE_SLOTS: &[ScenePhase10bTextureSlotContract] = &[
    ScenePhase10bTextureSlotContract {
        slot: 0,
        semantic: ScenePhase10bBindingSemantic::PreviousInput,
        uv_space: ScenePhase10bUvSpace::PrimaryInput,
        required: true,
    },
    ScenePhase10bTextureSlotContract {
        slot: 1,
        semantic: ScenePhase10bBindingSemantic::OpacityMask,
        uv_space: ScenePhase10bUvSpace::MaskTexture,
        required: false,
    },
    ScenePhase10bTextureSlotContract {
        slot: 2,
        semantic: ScenePhase10bBindingSemantic::TimeOffset,
        uv_space: ScenePhase10bUvSpace::AuxTexture,
        required: false,
    },
];

const TINT_TEXTURE_SLOTS: &[ScenePhase10bTextureSlotContract] = &[
    ScenePhase10bTextureSlotContract {
        slot: 0,
        semantic: ScenePhase10bBindingSemantic::PreviousInput,
        uv_space: ScenePhase10bUvSpace::PrimaryInput,
        required: true,
    },
    ScenePhase10bTextureSlotContract {
        slot: 1,
        semantic: ScenePhase10bBindingSemantic::OpacityMask,
        uv_space: ScenePhase10bUvSpace::MaskTexture,
        required: false,
    },
];

const SCROLL_TEXTURE_SLOTS: &[ScenePhase10bTextureSlotContract] =
    &[ScenePhase10bTextureSlotContract {
        slot: 0,
        semantic: ScenePhase10bBindingSemantic::PreviousInput,
        uv_space: ScenePhase10bUvSpace::PrimaryInput,
        required: true,
    }];

const LIGHTSHAFTS_TEXTURE_SLOTS: &[ScenePhase10bTextureSlotContract] = &[
    ScenePhase10bTextureSlotContract {
        slot: 0,
        semantic: ScenePhase10bBindingSemantic::PreviousInput,
        uv_space: ScenePhase10bUvSpace::PrimaryInput,
        required: true,
    },
    ScenePhase10bTextureSlotContract {
        slot: 1,
        semantic: ScenePhase10bBindingSemantic::NoiseTexture,
        uv_space: ScenePhase10bUvSpace::AuxTexture,
        required: true,
    },
    ScenePhase10bTextureSlotContract {
        slot: 2,
        semantic: ScenePhase10bBindingSemantic::GradientTexture,
        uv_space: ScenePhase10bUvSpace::AuxTexture,
        required: false,
    },
];

const FOLIAGESWAY_TEXTURE_SLOTS: &[ScenePhase10bTextureSlotContract] = &[
    ScenePhase10bTextureSlotContract {
        slot: 0,
        semantic: ScenePhase10bBindingSemantic::PreviousInput,
        uv_space: ScenePhase10bUvSpace::PrimaryInput,
        required: true,
    },
    ScenePhase10bTextureSlotContract {
        slot: 1,
        semantic: ScenePhase10bBindingSemantic::OpacityMask,
        uv_space: ScenePhase10bUvSpace::MaskTexture,
        required: false,
    },
    ScenePhase10bTextureSlotContract {
        slot: 2,
        semantic: ScenePhase10bBindingSemantic::NoiseTexture,
        uv_space: ScenePhase10bUvSpace::AuxTexture,
        required: false,
    },
];

const CIRCLE_TEXTURE_SLOTS: &[ScenePhase10bTextureSlotContract] = &[ScenePhase10bTextureSlotContract {
    slot: 0,
    semantic: ScenePhase10bBindingSemantic::PreviousInput,
    uv_space: ScenePhase10bUvSpace::PrimaryInput,
    required: true,
}];

const OPACITY_TEXTURE_SLOTS: &[ScenePhase10bTextureSlotContract] = &[
    ScenePhase10bTextureSlotContract {
        slot: 0,
        semantic: ScenePhase10bBindingSemantic::PreviousInput,
        uv_space: ScenePhase10bUvSpace::PrimaryInput,
        required: true,
    },
    ScenePhase10bTextureSlotContract {
        slot: 1,
        semantic: ScenePhase10bBindingSemantic::OpacityMask,
        uv_space: ScenePhase10bUvSpace::MaskTexture,
        required: false,
    },
];

const TRANSFORM_TEXTURE_SLOTS: &[ScenePhase10bTextureSlotContract] = &[ScenePhase10bTextureSlotContract {
    slot: 0,
    semantic: ScenePhase10bBindingSemantic::PreviousInput,
    uv_space: ScenePhase10bUvSpace::PrimaryInput,
    required: true,
}];

const SPIN_TEXTURE_SLOTS: &[ScenePhase10bTextureSlotContract] = &[
    ScenePhase10bTextureSlotContract {
        slot: 0,
        semantic: ScenePhase10bBindingSemantic::PreviousInput,
        uv_space: ScenePhase10bUvSpace::PrimaryInput,
        required: true,
    },
    ScenePhase10bTextureSlotContract {
        slot: 1,
        semantic: ScenePhase10bBindingSemantic::OpacityMask,
        uv_space: ScenePhase10bUvSpace::MaskTexture,
        required: false,
    },
];

const CHROMATIC_ABERRATION_TEXTURE_SLOTS: &[ScenePhase10bTextureSlotContract] = &[
    ScenePhase10bTextureSlotContract {
        slot: 0,
        semantic: ScenePhase10bBindingSemantic::PreviousInput,
        uv_space: ScenePhase10bUvSpace::PrimaryInput,
        required: true,
    },
    ScenePhase10bTextureSlotContract {
        slot: 1,
        semantic: ScenePhase10bBindingSemantic::OpacityMask,
        uv_space: ScenePhase10bUvSpace::MaskTexture,
        required: false,
    },
];

const THREE_SLOT_MASK_TEXTURE_SLOTS: &[ScenePhase10bTextureSlotContract] = &[
    ScenePhase10bTextureSlotContract {
        slot: 0,
        semantic: ScenePhase10bBindingSemantic::PreviousInput,
        uv_space: ScenePhase10bUvSpace::PrimaryInput,
        required: true,
    },
    ScenePhase10bTextureSlotContract {
        slot: 1,
        semantic: ScenePhase10bBindingSemantic::NoiseTexture,
        uv_space: ScenePhase10bUvSpace::AuxTexture,
        required: true,
    },
    ScenePhase10bTextureSlotContract {
        slot: 2,
        semantic: ScenePhase10bBindingSemantic::OpacityMask,
        uv_space: ScenePhase10bUvSpace::MaskTexture,
        required: false,
    },
];

const WATERFLOW_TEXTURE_SLOTS: &[ScenePhase10bTextureSlotContract] = &[
    ScenePhase10bTextureSlotContract {
        slot: 0,
        semantic: ScenePhase10bBindingSemantic::PreviousInput,
        uv_space: ScenePhase10bUvSpace::PrimaryInput,
        required: true,
    },
    ScenePhase10bTextureSlotContract {
        slot: 1,
        semantic: ScenePhase10bBindingSemantic::FlowMap,
        uv_space: ScenePhase10bUvSpace::AuxTexture,
        required: true,
    },
    ScenePhase10bTextureSlotContract {
        slot: 2,
        semantic: ScenePhase10bBindingSemantic::TimeOffset,
        uv_space: ScenePhase10bUvSpace::AuxTexture,
        required: true,
    },
];

const FOUR_SLOT_BLEND_TEXTURE_SLOTS: &[ScenePhase10bTextureSlotContract] = &[
    ScenePhase10bTextureSlotContract {
        slot: 0,
        semantic: ScenePhase10bBindingSemantic::PreviousInput,
        uv_space: ScenePhase10bUvSpace::PrimaryInput,
        required: true,
    },
    ScenePhase10bTextureSlotContract {
        slot: 1,
        semantic: ScenePhase10bBindingSemantic::GradientTexture,
        uv_space: ScenePhase10bUvSpace::AuxTexture,
        required: true,
    },
    ScenePhase10bTextureSlotContract {
        slot: 2,
        semantic: ScenePhase10bBindingSemantic::TimeOffset,
        uv_space: ScenePhase10bUvSpace::AuxTexture,
        required: false,
    },
    ScenePhase10bTextureSlotContract {
        slot: 3,
        semantic: ScenePhase10bBindingSemantic::OpacityMask,
        uv_space: ScenePhase10bUvSpace::MaskTexture,
        required: false,
    },
];

const SHIMMER_TEXTURE_SLOTS: &[ScenePhase10bTextureSlotContract] = &[
    ScenePhase10bTextureSlotContract {
        slot: 0,
        semantic: ScenePhase10bBindingSemantic::PreviousInput,
        uv_space: ScenePhase10bUvSpace::PrimaryInput,
        required: true,
    },
    ScenePhase10bTextureSlotContract {
        slot: 1,
        semantic: ScenePhase10bBindingSemantic::OpacityMask,
        uv_space: ScenePhase10bUvSpace::MaskTexture,
        required: false,
    },
    ScenePhase10bTextureSlotContract {
        slot: 2,
        semantic: ScenePhase10bBindingSemantic::TimeOffset,
        uv_space: ScenePhase10bUvSpace::AuxTexture,
        required: false,
    },
    ScenePhase10bTextureSlotContract {
        slot: 3,
        semantic: ScenePhase10bBindingSemantic::GradientTexture,
        uv_space: ScenePhase10bUvSpace::AuxTexture,
        required: true,
    },
];

const XRAY_TEXTURE_SLOTS: &[ScenePhase10bTextureSlotContract] = &[
    ScenePhase10bTextureSlotContract {
        slot: 0,
        semantic: ScenePhase10bBindingSemantic::PreviousInput,
        uv_space: ScenePhase10bUvSpace::PrimaryInput,
        required: true,
    },
    ScenePhase10bTextureSlotContract {
        slot: 1,
        semantic: ScenePhase10bBindingSemantic::GradientTexture,
        uv_space: ScenePhase10bUvSpace::AuxTexture,
        required: true,
    },
    ScenePhase10bTextureSlotContract {
        slot: 2,
        semantic: ScenePhase10bBindingSemantic::SpriteTexture,
        uv_space: ScenePhase10bUvSpace::AuxTexture,
        required: true,
    },
    ScenePhase10bTextureSlotContract {
        slot: 3,
        semantic: ScenePhase10bBindingSemantic::OpacityMask,
        uv_space: ScenePhase10bUvSpace::MaskTexture,
        required: false,
    },
];

const BLEND_TEXTURE_SLOTS: &[ScenePhase10bTextureSlotContract] = &[
    ScenePhase10bTextureSlotContract {
        slot: 0,
        semantic: ScenePhase10bBindingSemantic::PreviousInput,
        uv_space: ScenePhase10bUvSpace::PrimaryInput,
        required: true,
    },
    ScenePhase10bTextureSlotContract {
        slot: 1,
        semantic: ScenePhase10bBindingSemantic::GradientTexture,
        uv_space: ScenePhase10bUvSpace::AuxTexture,
        required: true,
    },
    ScenePhase10bTextureSlotContract {
        slot: 7,
        semantic: ScenePhase10bBindingSemantic::OpacityMask,
        uv_space: ScenePhase10bUvSpace::MaskTexture,
        required: false,
    },
];

const WATERCAUSTICS_TEXTURE_SLOTS: &[ScenePhase10bTextureSlotContract] = &[
    ScenePhase10bTextureSlotContract {
        slot: 0,
        semantic: ScenePhase10bBindingSemantic::PreviousInput,
        uv_space: ScenePhase10bUvSpace::PrimaryInput,
        required: true,
    },
    ScenePhase10bTextureSlotContract {
        slot: 1,
        semantic: ScenePhase10bBindingSemantic::OpacityMask,
        uv_space: ScenePhase10bUvSpace::MaskTexture,
        required: false,
    },
    ScenePhase10bTextureSlotContract {
        slot: 2,
        semantic: ScenePhase10bBindingSemantic::GradientTexture,
        uv_space: ScenePhase10bUvSpace::AuxTexture,
        required: true,
    },
    ScenePhase10bTextureSlotContract {
        slot: 3,
        semantic: ScenePhase10bBindingSemantic::NoiseTexture,
        uv_space: ScenePhase10bUvSpace::AuxTexture,
        required: false,
    },
    ScenePhase10bTextureSlotContract {
        slot: 4,
        semantic: ScenePhase10bBindingSemantic::FlowMap,
        uv_space: ScenePhase10bUvSpace::AuxTexture,
        required: false,
    },
    ScenePhase10bTextureSlotContract {
        slot: 5,
        semantic: ScenePhase10bBindingSemantic::GradientTexture,
        uv_space: ScenePhase10bUvSpace::AuxTexture,
        required: false,
    },
];

const PULSE_CONTRACT: ScenePhase10bEffectContract = ScenePhase10bEffectContract {
    kind: SceneCompatEffectKind::Pulse,
    family: "pulse",
    required_texture_slots: &[0],
    supported_texture_slots: &[0, 1, 2],
    supported_combo_defaults: &[
        ("AUDIOPROCESSING", 0),
        ("BLENDMODE", 9),
        ("MASK", 0),
        ("PULSEALPHA", 0),
        ("PULSECOLOR", 1),
    ],
    supported_uniforms: &[
        "amount",
        "bounds",
        "noiseamount",
        "noisespeed",
        "phase",
        "power",
        "speed",
        "tinthigh",
        "tintlow",
    ],
    runtime_binding_layout: PULSE_TEXTURE_SLOTS,
};

const SHAKE_CONTRACT: ScenePhase10bEffectContract = ScenePhase10bEffectContract {
    kind: SceneCompatEffectKind::Shake,
    family: "shake",
    required_texture_slots: &[0, 1],
    supported_texture_slots: &[0, 1, 2, 3],
    supported_combo_defaults: &[
        ("AUDIOPROCESSING", 0),
        ("DIRECTION", 0),
        ("MASK", 0),
        ("NOISE", 0),
        ("TIMEOFFSET", 0),
    ],
    supported_uniforms: &["bounds", "friction", "speed", "strength"],
    runtime_binding_layout: SHAKE_TEXTURE_SLOTS,
};

const WATERRIPPLE_CONTRACT: ScenePhase10bEffectContract = ScenePhase10bEffectContract {
    kind: SceneCompatEffectKind::WaterRipple,
    family: "waterripple",
    required_texture_slots: &[0, 2],
    supported_texture_slots: &[0, 1, 2],
    supported_combo_defaults: &[("MASK", 0), ("PERSPECTIVE", 0), ("SPECULAR", 0)],
    supported_uniforms: &[
        "animationspeed",
        "ratio",
        "ripplestrength",
        "scale",
        "scrolldirection",
        "scrollspeed",
    ],
    runtime_binding_layout: WATERRIPPLE_TEXTURE_SLOTS,
};

const WATERWAVES_CONTRACT: ScenePhase10bEffectContract = ScenePhase10bEffectContract {
    kind: SceneCompatEffectKind::WaterWaves,
    family: "waterwaves",
    required_texture_slots: &[0],
    supported_texture_slots: &[0, 1, 2],
    supported_combo_defaults: &[
        ("DUALWAVES", 0),
        ("MASK", 0),
        ("PERSPECTIVE", 0),
        ("TIMEOFFSET", 0),
    ],
    supported_uniforms: &["direction", "exponent", "scale", "speed", "strength"],
    runtime_binding_layout: WATERWAVES_TEXTURE_SLOTS,
};

const TINT_CONTRACT: ScenePhase10bEffectContract = ScenePhase10bEffectContract {
    kind: SceneCompatEffectKind::Tint,
    family: "tint",
    required_texture_slots: &[0],
    supported_texture_slots: &[0, 1],
    supported_combo_defaults: &[("BLENDMODE", 30), ("MASK", 0)],
    supported_uniforms: &["alpha", "color"],
    runtime_binding_layout: TINT_TEXTURE_SLOTS,
};

const SCROLL_CONTRACT: ScenePhase10bEffectContract = ScenePhase10bEffectContract {
    kind: SceneCompatEffectKind::Scroll,
    family: "scroll",
    required_texture_slots: &[0],
    supported_texture_slots: &[0],
    supported_combo_defaults: &[],
    supported_uniforms: &["repeat", "speedx", "speedy"],
    runtime_binding_layout: SCROLL_TEXTURE_SLOTS,
};

const LIGHTSHAFTS_CONTRACT: ScenePhase10bEffectContract = ScenePhase10bEffectContract {
    kind: SceneCompatEffectKind::LightShafts,
    family: "lightshafts",
    required_texture_slots: &[0],
    supported_texture_slots: &[0, 1, 2],
    supported_combo_defaults: &[
        ("BLENDMODE", 31),
        ("DIRECTDRAW", 0),
        ("RAYMODE", 0),
        ("RENDERING", 0),
        ("WRITEALPHA", 0),
    ],
    supported_uniforms: &[
        "colorastart",
        "colorend",
        "colorwexponent",
        "colorwintensity",
        "noiseamount",
        "noisescale",
        "point0",
        "point1",
        "point2",
        "point3",
        "rayfeather",
        "rayradius",
        "rayscale",
        "raysmoothness",
        "rayspeed",
    ],
    runtime_binding_layout: LIGHTSHAFTS_TEXTURE_SLOTS,
};

const FOLIAGESWAY_CONTRACT: ScenePhase10bEffectContract = ScenePhase10bEffectContract {
    kind: SceneCompatEffectKind::FoliageSway,
    family: "foliagesway",
    required_texture_slots: &[0],
    supported_texture_slots: &[0, 1, 2],
    supported_combo_defaults: &[("MASK", 0), ("MODE", 0)],
    supported_uniforms: &["phase", "power", "speed", "strength"],
    runtime_binding_layout: FOLIAGESWAY_TEXTURE_SLOTS,
};

const CIRCLE_CONTRACT: ScenePhase10bEffectContract = ScenePhase10bEffectContract {
    kind: SceneCompatEffectKind::Circle,
    family: "circle",
    required_texture_slots: &[0],
    supported_texture_slots: &[0],
    supported_combo_defaults: &[],
    supported_uniforms: &[],
    runtime_binding_layout: CIRCLE_TEXTURE_SLOTS,
};

const OPACITY_CONTRACT: ScenePhase10bEffectContract = ScenePhase10bEffectContract {
    kind: SceneCompatEffectKind::Opacity,
    family: "opacity",
    required_texture_slots: &[0],
    supported_texture_slots: &[0, 1],
    supported_combo_defaults: &[("MASK", 0)],
    supported_uniforms: &["alpha", "useralpha"],
    runtime_binding_layout: OPACITY_TEXTURE_SLOTS,
};

const TRANSFORM_CONTRACT: ScenePhase10bEffectContract = ScenePhase10bEffectContract {
    kind: SceneCompatEffectKind::Transform,
    family: "transform",
    required_texture_slots: &[0],
    supported_texture_slots: &[0],
    supported_combo_defaults: &[("CLAMP", 1)],
    supported_uniforms: &[],
    runtime_binding_layout: TRANSFORM_TEXTURE_SLOTS,
};

const SKEW_CONTRACT: ScenePhase10bEffectContract = ScenePhase10bEffectContract {
    kind: SceneCompatEffectKind::Skew,
    family: "skew",
    required_texture_slots: &[0],
    supported_texture_slots: &[0],
    supported_combo_defaults: &[("REPEAT", 1)],
    supported_uniforms: &[],
    runtime_binding_layout: TRANSFORM_TEXTURE_SLOTS,
};

const PERSPECTIVE_CONTRACT: ScenePhase10bEffectContract = ScenePhase10bEffectContract {
    kind: SceneCompatEffectKind::Perspective,
    family: "perspective",
    required_texture_slots: &[0],
    supported_texture_slots: &[0],
    supported_combo_defaults: &[("REPEAT", 0)],
    supported_uniforms: &["point0", "point1", "point2", "point3"],
    runtime_binding_layout: TRANSFORM_TEXTURE_SLOTS,
};

const SPIN_CONTRACT: ScenePhase10bEffectContract = ScenePhase10bEffectContract {
    kind: SceneCompatEffectKind::Spin,
    family: "spin",
    required_texture_slots: &[0],
    supported_texture_slots: &[0, 1],
    supported_combo_defaults: &[("MASK", 0), ("REPEAT", 1)],
    supported_uniforms: &["center", "feather", "size", "spincenter"],
    runtime_binding_layout: SPIN_TEXTURE_SLOTS,
};

const SWING_CONTRACT: ScenePhase10bEffectContract = ScenePhase10bEffectContract {
    kind: SceneCompatEffectKind::Swing,
    family: "swing",
    required_texture_slots: &[0],
    supported_texture_slots: &[0, 1, 2],
    supported_combo_defaults: &[("DOUBLESIDED", 0), ("MASK", 0), ("NOISE", 0)],
    supported_uniforms: &[
        "amount",
        "center",
        "feather",
        "noiseamount",
        "noisespeed",
        "point0",
        "point1",
        "size",
        "speed",
    ],
    runtime_binding_layout: THREE_SLOT_MASK_TEXTURE_SLOTS,
};

const TWIRL_CONTRACT: ScenePhase10bEffectContract = ScenePhase10bEffectContract {
    kind: SceneCompatEffectKind::Twirl,
    family: "twirl",
    required_texture_slots: &[0],
    supported_texture_slots: &[0, 1, 2],
    supported_combo_defaults: &[
        ("ELLIPTICAL", 1),
        ("INNER", 0),
        ("MASK", 0),
        ("NOISE", 0),
        ("REPEAT", 1),
    ],
    supported_uniforms: &[
        "amount",
        "angle",
        "center",
        "feather",
        "noiseamount",
        "noisespeed",
        "ratio",
        "size",
        "speed",
    ],
    runtime_binding_layout: THREE_SLOT_MASK_TEXTURE_SLOTS,
};

const CHROMATIC_ABERRATION_CONTRACT: ScenePhase10bEffectContract = ScenePhase10bEffectContract {
    kind: SceneCompatEffectKind::ChromaticAberration,
    family: "chromaticaberration",
    required_texture_slots: &[0],
    supported_texture_slots: &[0, 1],
    supported_combo_defaults: &[("MASK", 0), ("MODE", 0), ("VARIATION", 0)],
    supported_uniforms: &[
        "uieditorpropertiescenter",
        "uieditorpropertiescenterfalloff",
        "uieditorpropertiesdirection",
        "uieditorpropertiesstrength",
    ],
    runtime_binding_layout: CHROMATIC_ABERRATION_TEXTURE_SLOTS,
};

const COLORKEY_CONTRACT: ScenePhase10bEffectContract = ScenePhase10bEffectContract {
    kind: SceneCompatEffectKind::ColorKey,
    family: "colorkey",
    required_texture_slots: &[0],
    supported_texture_slots: &[0],
    supported_combo_defaults: &[("FLATTEN", 0), ("INVERT", 0)],
    supported_uniforms: &["alpha", "color", "fuzziness", "tolerance"],
    runtime_binding_layout: TRANSFORM_TEXTURE_SLOTS,
};

const FISHEYE_CONTRACT: ScenePhase10bEffectContract = ScenePhase10bEffectContract {
    kind: SceneCompatEffectKind::FishEye,
    family: "fisheye",
    required_texture_slots: &[0],
    supported_texture_slots: &[0],
    supported_combo_defaults: &[("BACKGROUND", 1)],
    supported_uniforms: &["center", "distortion", "scale", "size"],
    runtime_binding_layout: TRANSFORM_TEXTURE_SLOTS,
};

const EDGEDETECTION_CONTRACT: ScenePhase10bEffectContract = ScenePhase10bEffectContract {
    kind: SceneCompatEffectKind::EdgeDetection,
    family: "edgedetection",
    required_texture_slots: &[0],
    supported_texture_slots: &[0],
    supported_combo_defaults: &[("BLENDMODE", 0)],
    supported_uniforms: &[
        "alpha",
        "brightness",
        "detectmultiply",
        "detectionmultiply",
        "detectionsize",
        "detectionthreshold",
        "outlinebackground",
        "outlinecolor",
    ],
    runtime_binding_layout: TRANSFORM_TEXTURE_SLOTS,
};

const IRIS_CONTRACT: ScenePhase10bEffectContract = ScenePhase10bEffectContract {
    kind: SceneCompatEffectKind::Iris,
    family: "iris",
    required_texture_slots: &[0],
    supported_texture_slots: &[0, 1],
    supported_combo_defaults: &[("BACKGROUND", 0), ("MASK", 0)],
    supported_uniforms: &["color", "scale"],
    runtime_binding_layout: OPACITY_TEXTURE_SLOTS,
};

const CLOUDMOTION_CONTRACT: ScenePhase10bEffectContract = ScenePhase10bEffectContract {
    kind: SceneCompatEffectKind::CloudMotion,
    family: "cloudmotion",
    required_texture_slots: &[0, 2],
    supported_texture_slots: &[0, 1, 2],
    supported_combo_defaults: &[("MASK", 0)],
    supported_uniforms: &[
        "uieditorpropertiesamount",
        "uieditorpropertiesdirection",
        "amount",
        "direction",
    ],
    runtime_binding_layout: THREE_SLOT_MASK_TEXTURE_SLOTS,
};

const CLOUDS_CONTRACT: ScenePhase10bEffectContract = ScenePhase10bEffectContract {
    kind: SceneCompatEffectKind::Clouds,
    family: "clouds",
    required_texture_slots: &[0, 1],
    supported_texture_slots: &[0, 1, 2],
    supported_combo_defaults: &[
        ("BLENDMODE", 0),
        ("MASK", 0),
        ("SHADING", 7),
        ("WRITEALPHA", 0),
    ],
    supported_uniforms: &[
        "alpha",
        "colorend",
        "colorstart",
        "feather",
        "scale",
        "smoothness",
        "speed",
        "threshold",
    ],
    runtime_binding_layout: THREE_SLOT_MASK_TEXTURE_SLOTS,
};

const WATERFLOW_CONTRACT: ScenePhase10bEffectContract = ScenePhase10bEffectContract {
    kind: SceneCompatEffectKind::WaterFlow,
    family: "waterflow",
    required_texture_slots: &[0, 1, 2],
    supported_texture_slots: &[0, 1, 2],
    supported_combo_defaults: &[],
    supported_uniforms: &["phasescale", "strength"],
    runtime_binding_layout: WATERFLOW_TEXTURE_SLOTS,
};

const NITRO_CONTRACT: ScenePhase10bEffectContract = ScenePhase10bEffectContract {
    kind: SceneCompatEffectKind::Nitro,
    family: "nitro",
    required_texture_slots: &[0, 1],
    supported_texture_slots: &[0, 1, 2],
    supported_combo_defaults: &[("BLENDMODE", 22), ("MASK", 0), ("WRITEALPHA", 0)],
    supported_uniforms: &["bounds", "colorend", "colorstart", "multiply", "smoothness"],
    runtime_binding_layout: THREE_SLOT_MASK_TEXTURE_SLOTS,
};

const BLEND_CONTRACT: ScenePhase10bEffectContract = ScenePhase10bEffectContract {
    kind: SceneCompatEffectKind::Blend,
    family: "blend",
    required_texture_slots: &[0, 1],
    supported_texture_slots: &[0, 1, 7],
    supported_combo_defaults: &[
        ("BLENDMODE", 2),
        ("NUMBLENDTEXTURES", 1),
        ("OPACITYMASK", 0),
        ("TRANSFORMREPEAT", 0),
        ("TRANSFORMUV", 0),
        ("WRITEALPHA", 0),
    ],
    supported_uniforms: &["alpha", "multiply"],
    runtime_binding_layout: BLEND_TEXTURE_SLOTS,
};

const REFLECTION_CONTRACT: ScenePhase10bEffectContract = ScenePhase10bEffectContract {
    kind: SceneCompatEffectKind::Reflection,
    family: "reflection",
    required_texture_slots: &[0],
    supported_texture_slots: &[0, 1],
    supported_combo_defaults: &[("BLENDMODE", 9), ("MASK", 0), ("PERSPECTIVE", 0)],
    supported_uniforms: &["alpha"],
    runtime_binding_layout: OPACITY_TEXTURE_SLOTS,
};

const SHIMMER_CONTRACT: ScenePhase10bEffectContract = ScenePhase10bEffectContract {
    kind: SceneCompatEffectKind::Shimmer,
    family: "shimmer",
    required_texture_slots: &[0, 3],
    supported_texture_slots: &[0, 1, 2, 3],
    supported_combo_defaults: &[("BLENDMODE", 32), ("MASK", 0), ("MODE", 0), ("OFFSET", 0)],
    supported_uniforms: &[
        "uieditorpropertiesamount",
        "uieditorpropertiesbrightness",
        "uieditorpropertiescolor",
        "uieditorpropertiesdelay",
        "uieditorpropertiesdirection",
        "uieditorpropertiesgranularity",
        "uieditorpropertiesoffset",
        "uieditorpropertiesspeed",
        "uieditorpropertiestimescale",
        "uieditorpropertieswidth",
    ],
    runtime_binding_layout: SHIMMER_TEXTURE_SLOTS,
};

const FILMGRAIN_CONTRACT: ScenePhase10bEffectContract = ScenePhase10bEffectContract {
    kind: SceneCompatEffectKind::FilmGrain,
    family: "filmgrain",
    required_texture_slots: &[0, 1],
    supported_texture_slots: &[0, 1, 2],
    supported_combo_defaults: &[("BLENDMODE", 12), ("GREYSCALE", 1), ("MASK", 0)],
    supported_uniforms: &[
        "exponent",
        "strength",
        "uieditorpropertiespower",
        "uieditorpropertiesstrength",
    ],
    runtime_binding_layout: THREE_SLOT_MASK_TEXTURE_SLOTS,
};

const VHS_CONTRACT: ScenePhase10bEffectContract = ScenePhase10bEffectContract {
    kind: SceneCompatEffectKind::Vhs,
    family: "vhs",
    required_texture_slots: &[0, 1],
    supported_texture_slots: &[0, 1, 2],
    supported_combo_defaults: &[
        ("BLENDMODE", 12),
        ("GREYSCALE", 0),
        ("INVERTARTIFACTS", 1),
        ("MASK", 0),
    ],
    supported_uniforms: &[
        "artifacts",
        "chromatic",
        "distortionspeed",
        "distortionstrength",
        "distortionwidth",
        "scale",
        "strength",
        "tracking",
    ],
    runtime_binding_layout: THREE_SLOT_MASK_TEXTURE_SLOTS,
};

const BLENDGRADIENT_CONTRACT: ScenePhase10bEffectContract = ScenePhase10bEffectContract {
    kind: SceneCompatEffectKind::BlendGradient,
    family: "blendgradient",
    required_texture_slots: &[0, 1, 2],
    supported_texture_slots: &[0, 1, 2, 3],
    supported_combo_defaults: &[
        ("BLENDMODE", 0),
        ("EDGEGLOW", 0),
        ("OPACITYMASK", 0),
        ("TRANSFORMREPEAT", 0),
        ("TRANSFORMUV", 0),
        ("WRITEALPHA", 0),
    ],
    supported_uniforms: &["alpha", "edgebrightness", "edgecolor", "gradientscale", "multiply"],
    runtime_binding_layout: FOUR_SLOT_BLEND_TEXTURE_SLOTS,
};

const WATERCAUSTICS_CONTRACT: ScenePhase10bEffectContract = ScenePhase10bEffectContract {
    kind: SceneCompatEffectKind::WaterCaustics,
    family: "watercaustics",
    required_texture_slots: &[0, 2],
    supported_texture_slots: &[0, 1, 2, 3, 4, 5],
    supported_combo_defaults: &[("BLENDMODE", 32), ("MASK", 0), ("MODE", 0), ("PERSPECTIVE", 0)],
    supported_uniforms: &[
        "uieditorpropertiesbrightness",
        "uieditorpropertiesblur",
        "uieditorpropertieschromaticaberration",
        "uieditorpropertiescolorend",
        "uieditorpropertiescolorstart",
        "uieditorpropertiesdistortion",
        "uieditorpropertiesglow",
        "uieditorpropertiesgranularity",
        "uieditorpropertiesspeed",
        "uieditorpropertiestimeoffset",
    ],
    runtime_binding_layout: WATERCAUSTICS_TEXTURE_SLOTS,
};

const XRAY_CONTRACT: ScenePhase10bEffectContract = ScenePhase10bEffectContract {
    kind: SceneCompatEffectKind::XRay,
    family: "xray",
    required_texture_slots: &[0, 1, 2],
    supported_texture_slots: &[0, 1, 2, 3],
    supported_combo_defaults: &[("BLENDMODE", 0), ("OPACITYMASK", 0)],
    supported_uniforms: &[
        "multiply",
        "uieditorpropertiesmultiply",
        "uieditorparticleelementexponent",
    ],
    runtime_binding_layout: XRAY_TEXTURE_SLOTS,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SceneShaderProgram {
    pub key: String,
    pub kind: SceneShaderProgramKind,
    pub metal_source_path: PathBuf,
    pub vertex_entry: &'static str,
    pub fragment_entry: &'static str,
    pub variant_defines: BTreeMap<String, i32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SceneMaterialUniformValue {
    Float(u32),
    Float2([u32; 2]),
    Float3([u32; 3]),
    Float4([u32; 4]),
}

impl SceneMaterialUniformValue {
    pub fn as_float(&self) -> Option<f32> {
        match self {
            Self::Float(value) => Some(f32::from_bits(*value)),
            _ => None,
        }
    }

    pub fn as_float2(&self) -> Option<[f32; 2]> {
        match self {
            Self::Float2(value) => Some([f32::from_bits(value[0]), f32::from_bits(value[1])]),
            _ => None,
        }
    }

    pub fn as_float3(&self) -> Option<[f32; 3]> {
        match self {
            Self::Float3(value) => Some([
                f32::from_bits(value[0]),
                f32::from_bits(value[1]),
                f32::from_bits(value[2]),
            ]),
            _ => None,
        }
    }

    pub fn as_float4(&self) -> Option<[f32; 4]> {
        match self {
            Self::Float4(value) => Some([
                f32::from_bits(value[0]),
                f32::from_bits(value[1]),
                f32::from_bits(value[2]),
                f32::from_bits(value[3]),
            ]),
            _ => None,
        }
    }

    fn from_floats(values: &[f32]) -> Option<Self> {
        match values {
            [x] => Some(Self::Float(x.to_bits())),
            [x, y] => Some(Self::Float2([x.to_bits(), y.to_bits()])),
            [x, y, z] => Some(Self::Float3([x.to_bits(), y.to_bits(), z.to_bits()])),
            [x, y, z, w] => Some(Self::Float4([
                x.to_bits(),
                y.to_bits(),
                z.to_bits(),
                w.to_bits(),
            ])),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SceneMaterialTextureBinding {
    pub slot_index: usize,
    pub slot_name: String,
    pub texture_name: Option<String>,
    pub resolved_path: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SceneMaterialPassPlan {
    pub index: usize,
    pub shader_ref: String,
    pub program: SceneShaderProgram,
    pub blend_mode: SceneRenderBlendMode,
    pub combos: BTreeMap<String, i32>,
    pub uniforms: BTreeMap<String, SceneMaterialUniformValue>,
    pub textures: Vec<SceneMaterialTextureBinding>,
    pub effect_paths: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SceneEffectBinding {
    pub index: usize,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SceneEffectPassPlan {
    pub index: usize,
    pub material_path: Option<String>,
    pub material_lookup: Option<SceneResourceLookup>,
    pub target_name: Option<String>,
    pub copy_background: bool,
    pub bindings: Vec<SceneEffectBinding>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SceneEffectPlan {
    pub effect_path: PathBuf,
    pub effect_package_root: PathBuf,
    pub version: Option<i64>,
    pub fbo_names: Vec<String>,
    pub copy_background: bool,
    pub shader_dependencies: Vec<String>,
    pub dependency_lookups: Vec<SceneResourceLookup>,
    pub passes: Vec<SceneEffectPassPlan>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SceneResolvedMaterialPlan {
    pub material_path: PathBuf,
    pub passes: Vec<SceneMaterialPassPlan>,
    pub material_effects: Vec<SceneEffectPlan>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SceneMaterialSummary {
    pub material_path: PathBuf,
    pub pass_count: usize,
    pub max_texture_count: usize,
    pub has_combos: bool,
    pub has_effects: bool,
    pub requires_phase10_graph: bool,
}

pub fn load_scene_material_plan(
    resolver: &SceneResourceResolver,
    material_path: &str,
) -> Result<SceneResolvedMaterialPlan, String> {
    let lookup = resolver.inspect_relative_path(material_path);
    let resolved_path = lookup.matched_path.clone().ok_or_else(|| {
        format!("material {material_path} could not be resolved from Scene roots")
    })?;
    load_scene_material_plan_from_resolved_path(resolver, material_path, &resolved_path, None)
}

pub fn load_scene_material_plan_with_effect_package_root(
    resolver: &SceneResourceResolver,
    material_path: &str,
    effect_package_root: &Path,
) -> Result<SceneResolvedMaterialPlan, String> {
    let lookup = resolver.inspect_relative_path_with_local_root(
        material_path,
        SceneResourceRootKind::EffectPackage,
        effect_package_root,
    );
    let resolved_path = lookup.matched_path.clone().ok_or_else(|| {
        format!(
            "material {material_path} could not be resolved from effect package {} or Scene roots",
            effect_package_root.display()
        )
    })?;
    load_scene_material_plan_from_resolved_path(
        resolver,
        material_path,
        &resolved_path,
        Some(effect_package_root),
    )
}

fn load_scene_material_plan_from_resolved_path(
    resolver: &SceneResourceResolver,
    material_path: &str,
    resolved_path: &Path,
    effect_package_root: Option<&Path>,
) -> Result<SceneResolvedMaterialPlan, String> {
    let json = read_json(resolved_path)?;
    let passes = material_pass_values(&json);
    if passes.is_empty() {
        return Err(format!(
            "material {} has no render passes",
            resolved_path.display()
        ));
    }

    let mut compiled_passes = Vec::with_capacity(passes.len());
    for (index, pass) in passes.iter().enumerate() {
        let shader_ref = pass
            .get("shader")
            .and_then(Value::as_str)
            .or_else(|| json.get("shader").and_then(Value::as_str))
            .ok_or_else(|| {
                format!(
                    "material {} pass {} does not declare a shader",
                    resolved_path.display(),
                    index
                )
            })?
            .to_string();
        let combos = parse_combo_map(pass.get("combos").or_else(|| json.get("combos")));
        let uniforms = parse_material_uniform_map(pass, &json);
        let program = resolve_shader_program_with_context(
            resolver,
            &shader_ref,
            &combos,
            effect_package_root,
        )?;
        let texture_names =
            parse_texture_list(pass.get("textures").or_else(|| json.get("textures")));
        let textures = texture_names
            .into_iter()
            .enumerate()
            .map(|(slot, name)| SceneMaterialTextureBinding {
                slot_index: slot,
                slot_name: format!("g_Texture{slot}"),
                resolved_path: name.as_deref().and_then(|name| {
                    resolve_texture_candidates_for_material(
                        resolver,
                        material_path,
                        resolved_path,
                        effect_package_root,
                        name,
                    )
                    .into_iter()
                    .next()
                }),
                texture_name: name,
            })
            .collect::<Vec<_>>();
        compiled_passes.push(SceneMaterialPassPlan {
            index,
            shader_ref,
            program,
            blend_mode: parse_material_blend_mode(
                pass.get("blending")
                    .or_else(|| json.get("blending"))
                    .and_then(Value::as_str),
            ),
            combos,
            uniforms,
            textures,
            effect_paths: effect_paths(pass)
                .into_iter()
                .chain(effect_paths(&json))
                .collect(),
        });
    }

    let material_effects = effect_paths(&json)
        .into_iter()
        .map(|path| {
            if let Some(effect_package_root) = effect_package_root {
                load_scene_effect_plan_with_local_root(resolver, &path, effect_package_root)
            } else {
                load_scene_effect_plan(resolver, &path)
            }
        })
        .collect::<Result<Vec<_>, _>>()?;

    Ok(SceneResolvedMaterialPlan {
        material_path: resolved_path.to_path_buf(),
        passes: compiled_passes,
        material_effects,
    })
}

pub fn inspect_scene_material_summary(
    resolver: &SceneResourceResolver,
    material_path: &str,
) -> Result<SceneMaterialSummary, String> {
    let resolved_path = resolver
        .resolve_relative_path(material_path)
        .ok_or_else(|| {
            format!("material {material_path} could not be resolved from Scene roots")
        })?;
    let json = read_json(&resolved_path)?;
    let passes = material_pass_values(&json);
    if passes.is_empty() {
        return Err(format!(
            "material {} has no render passes",
            resolved_path.display()
        ));
    }

    let mut max_texture_count = 0;
    let mut has_combos = false;
    let mut requires_phase10_graph = false;
    for pass in &passes {
        let textures = parse_texture_list(pass.get("textures").or_else(|| json.get("textures")));
        max_texture_count = max_texture_count.max(textures.len());
        let combos = parse_combo_map(pass.get("combos").or_else(|| json.get("combos")));
        has_combos |= !combos.is_empty();
        let shader_ref = pass
            .get("shader")
            .and_then(Value::as_str)
            .or_else(|| json.get("shader").and_then(Value::as_str));
        requires_phase10_graph |= shader_ref
            .map(shader_ref_requires_phase10_graph_semantics)
            .unwrap_or(false);
    }

    let has_effects = !effect_paths(&json).is_empty();
    requires_phase10_graph |=
        passes.len() > 1 || has_combos || has_effects || max_texture_count > 1;

    Ok(SceneMaterialSummary {
        material_path: resolved_path,
        pass_count: passes.len(),
        max_texture_count,
        has_combos,
        has_effects,
        requires_phase10_graph,
    })
}

pub fn load_scene_effect_plan(
    resolver: &SceneResourceResolver,
    effect_path: &str,
) -> Result<SceneEffectPlan, String> {
    let lookup = resolver.inspect_relative_path(effect_path);
    let resolved_path = lookup
        .matched_path
        .clone()
        .ok_or_else(|| format!("effect {effect_path} could not be resolved from Scene roots"))?;
    load_scene_effect_plan_from_resolved_path(resolver, effect_path, &resolved_path)
}

fn load_scene_effect_plan_with_local_root(
    resolver: &SceneResourceResolver,
    effect_path: &str,
    local_root: &Path,
) -> Result<SceneEffectPlan, String> {
    let lookup = resolver.inspect_relative_path_with_local_root(
        effect_path,
        SceneResourceRootKind::EffectPackage,
        local_root,
    );
    let resolved_path = lookup.matched_path.clone().ok_or_else(|| {
        format!(
            "effect {effect_path} could not be resolved from effect package {} or Scene roots",
            local_root.display()
        )
    })?;
    load_scene_effect_plan_from_resolved_path(resolver, effect_path, &resolved_path)
}

fn load_scene_effect_plan_from_resolved_path(
    resolver: &SceneResourceResolver,
    _authored_effect_path: &str,
    resolved_path: &Path,
) -> Result<SceneEffectPlan, String> {
    let json = read_json(resolved_path)?;
    let effect_package_root = resolved_path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(PathBuf::new);
    let passes = json
        .get("passes")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let shader_dependencies = json
        .get("dependencies")
        .and_then(Value::as_array)
        .map(|dependencies| {
            dependencies
                .iter()
                .filter_map(Value::as_str)
                .map(ToString::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let dependency_lookups = shader_dependencies
        .iter()
        .map(|dependency| {
            inspect_scene_effect_dependency(resolver, dependency, &effect_package_root)
        })
        .collect::<Vec<_>>();

    Ok(SceneEffectPlan {
        effect_path: resolved_path.to_path_buf(),
        effect_package_root: effect_package_root.clone(),
        version: json.get("version").and_then(Value::as_i64),
        fbo_names: json
            .get("fbos")
            .and_then(Value::as_array)
            .map(|entries| {
                entries
                    .iter()
                    .filter_map(|entry| entry.get("name").and_then(Value::as_str))
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default(),
        copy_background: json_declares_copy_background(&json),
        shader_dependencies,
        dependency_lookups,
        passes: passes
            .iter()
            .enumerate()
            .map(|(index, pass)| SceneEffectPassPlan {
                index,
                material_path: pass
                    .get("material")
                    .and_then(Value::as_str)
                    .map(ToString::to_string),
                material_lookup: pass
                    .get("material")
                    .and_then(Value::as_str)
                    .map(|material| {
                        resolver.inspect_relative_path_with_local_root(
                            material,
                            SceneResourceRootKind::EffectPackage,
                            &effect_package_root,
                        )
                    }),
                bindings: pass
                    .get("bind")
                    .and_then(Value::as_array)
                    .map(|entries| {
                        entries
                            .iter()
                            .filter_map(|entry| {
                                Some(SceneEffectBinding {
                                    index: entry.get("index")?.as_u64()? as usize,
                                    name: entry.get("name")?.as_str()?.to_string(),
                                })
                            })
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default(),
                target_name: pass
                    .get("target")
                    .and_then(Value::as_str)
                    .map(ToString::to_string),
                copy_background: json_declares_copy_background(pass),
            })
            .collect(),
    })
}

fn json_declares_copy_background(value: &Value) -> bool {
    ["copybackground", "copyBackground", "copy_background"]
        .iter()
        .any(|key| value.get(*key).and_then(Value::as_bool).unwrap_or(false))
}

pub fn inspect_scene_effect_dependency(
    resolver: &SceneResourceResolver,
    dependency: &str,
    effect_package_root: &Path,
) -> SceneResourceLookup {
    if effect_dependency_is_texture(dependency) {
        return resolver
            .inspect_texture_candidates_with_local_root(
                None,
                None,
                dependency,
                SceneResourceRootKind::EffectPackage,
                effect_package_root,
            )
            .lookup;
    }
    if effect_dependency_is_shader(dependency) {
        return inspect_scene_shader_source_with_effect_context(
            resolver,
            dependency,
            Some(effect_package_root),
        )
        .lookup;
    }
    resolver.inspect_relative_path_with_local_root(
        dependency,
        SceneResourceRootKind::EffectPackage,
        effect_package_root,
    )
}

fn effect_dependency_is_shader(dependency: &str) -> bool {
    let lower = dependency.to_ascii_lowercase();
    lower.starts_with("shaders/")
        || lower.contains("/shaders/")
        || lower.ends_with(".vert")
        || lower.ends_with(".frag")
        || lower.ends_with(".metal")
        || (Path::new(&lower).extension().is_none()
            && !lower.starts_with("materials/")
            && !lower.starts_with("textures/")
            && !lower.starts_with("preview/"))
}

fn effect_dependency_is_texture(dependency: &str) -> bool {
    let lower = dependency.to_ascii_lowercase();
    lower.ends_with(".tex")
        || lower.ends_with(".tex-json")
        || lower.ends_with(".tex.json")
        || lower.ends_with(".png")
        || lower.ends_with(".jpg")
        || lower.ends_with(".jpeg")
        || lower.ends_with(".webp")
        || lower.ends_with(".gif")
        || lower.ends_with(".tga")
        || lower.ends_with(".bmp")
}

pub fn load_shader_program_source(
    program: &SceneShaderProgram,
    defines: &BTreeMap<String, i32>,
) -> Result<String, String> {
    let source = fs::read_to_string(&program.metal_source_path).map_err(|error| {
        format!(
            "unable to read shader source {}: {error}",
            program.metal_source_path.display()
        )
    })?;
    Ok(preprocess_scene_shader_source(
        &source,
        &merged_shader_defines(program, defines),
    ))
}

pub fn preprocess_scene_shader_source(source: &str, defines: &BTreeMap<String, i32>) -> String {
    let mut normalized = source.replace("\r\n", "\n").replace('\r', "\n");
    if normalized.contains('；') {
        normalized = normalized.replace('；', ";");
    }

    let stripped = normalized
        .lines()
        .map(|line| {
            let trimmed = line.trim();
            if trimmed.starts_with("#include") || trimmed.starts_with("#require") {
                format!("// {trimmed}")
            } else if line.contains("[COMBO]")
                || (line.contains("uniform sampler2D") && line.contains('{'))
            {
                line.split('{').next().unwrap_or("").trim_end().to_string()
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n");

    if defines.is_empty() {
        stripped
    } else {
        let prefix = defines
            .iter()
            .map(|(key, value)| format!("#define {key} {value}"))
            .collect::<Vec<_>>()
            .join("\n");
        format!("{prefix}\n{stripped}")
    }
}

#[cfg_attr(not(test), allow(dead_code))]
pub fn resolve_shader_program(
    resolver: &SceneResourceResolver,
    shader_ref: &str,
    combos: &BTreeMap<String, i32>,
) -> Result<SceneShaderProgram, String> {
    resolve_shader_program_with_context(resolver, shader_ref, combos, None)
}

#[cfg_attr(not(test), allow(dead_code))]
pub fn resolve_shader_program_with_effect_package_root(
    resolver: &SceneResourceResolver,
    shader_ref: &str,
    combos: &BTreeMap<String, i32>,
    effect_package_root: &Path,
) -> Result<SceneShaderProgram, String> {
    resolve_shader_program_with_context(resolver, shader_ref, combos, Some(effect_package_root))
}

pub fn inspect_scene_shader_source_with_effect_context(
    resolver: &SceneResourceResolver,
    shader_ref: &str,
    effect_package_root: Option<&Path>,
) -> SceneShaderSourceLookup {
    if let Some(effect_package_root) = effect_package_root {
        resolver.inspect_shader_source_with_local_root(
            shader_ref,
            SceneResourceRootKind::EffectPackage,
            effect_package_root,
        )
    } else {
        resolver.inspect_shader_source(shader_ref)
    }
}

fn resolve_shader_program_with_context(
    resolver: &SceneResourceResolver,
    shader_ref: &str,
    combos: &BTreeMap<String, i32>,
    effect_package_root: Option<&Path>,
) -> Result<SceneShaderProgram, String> {
    let shader_ref = shader_ref.trim();
    if shader_ref.is_empty() {
        return Err("shader reference is empty".to_string());
    }

    if shader_ref.ends_with(".metal") || shader_ref.contains('/') || shader_ref.contains('\\') {
        let lookup = inspect_scene_shader_source_with_effect_context(
            resolver,
            shader_ref,
            effect_package_root,
        );
        return match lookup.kind {
            SceneShaderSourceKind::Metal => {
                let path = lookup.metal_source_path.ok_or_else(|| {
                    format!("shader {shader_ref} resolved without a metal source")
                })?;
                Ok(SceneShaderProgram {
                    key: path.display().to_string(),
                    kind: SceneShaderProgramKind::Sprite,
                    metal_source_path: path,
                    vertex_entry: "compat_sprite_vertex",
                    fragment_entry: "compat_sprite_fragment",
                    variant_defines: BTreeMap::new(),
                })
            }
            SceneShaderSourceKind::AuthoredSourceSet => {
                if let Some(program) =
                    resolve_supported_authored_effect_program(resolver, shader_ref, combos)?
                {
                    Ok(program)
                } else {
                    Err(unsupported_authored_shader_message(
                        shader_ref,
                        effect_package_root,
                        &lookup,
                    ))
                }
            }
            SceneShaderSourceKind::Missing => {
                Err(unresolved_shader_message(shader_ref, effect_package_root))
            }
        };
    }

    let lower = shader_ref.to_ascii_lowercase();
    let (kind, asset_path) = if lower.contains("clippingmaskimage4") {
        (
            SceneShaderProgramKind::MaskAlpha,
            "assets/shaders/compat/scene-mask-alpha.metal",
        )
    } else if combos.get("CLIPPINGTARGET").copied() == Some(1) {
        (
            SceneShaderProgramKind::MaskApply,
            "assets/shaders/compat/scene-mask-apply.metal",
        )
    } else if lower.contains("copy") {
        (
            SceneShaderProgramKind::Copy,
            "assets/shaders/compat/scene-copy.metal",
        )
    } else if lower.contains("model") || lower.contains("puppet") {
        (
            SceneShaderProgramKind::Model,
            "assets/shaders/compat/scene-model.metal",
        )
    } else if lower.contains("genericimage")
        || lower.contains("image")
        || lower.contains("sprite")
        || lower.contains("default")
        || lower.contains("textured")
    {
        (
            SceneShaderProgramKind::Sprite,
            "assets/shaders/compat/scene-sprite.metal",
        )
    } else {
        return Err(format!(
            "shader {shader_ref} does not map to a supported phase-10 compatibility program"
        ));
    };
    let metal_source_path = resolver
        .resolve_relative_path(asset_path)
        .ok_or_else(|| format!("built-in Scene shader asset {asset_path} could not be resolved"))?;
    Ok(SceneShaderProgram {
        key: format!("{shader_ref}:{:?}", kind),
        kind,
        metal_source_path,
        vertex_entry: shader_program_vertex_entry(kind),
        fragment_entry: shader_program_fragment_entry(kind),
        variant_defines: shader_program_base_defines(kind),
    })
}

pub fn merged_shader_defines(
    program: &SceneShaderProgram,
    defines: &BTreeMap<String, i32>,
) -> BTreeMap<String, i32> {
    let mut merged = program.variant_defines.clone();
    for (name, value) in defines {
        merged.insert(name.clone(), *value);
    }
    merged
}

fn unresolved_shader_message(shader_ref: &str, effect_package_root: Option<&Path>) -> String {
    if let Some(effect_package_root) = effect_package_root {
        format!(
            "shader {shader_ref} could not be resolved from effect package {} or Scene roots",
            effect_package_root.display()
        )
    } else {
        format!("shader {shader_ref} could not be resolved from Scene roots")
    }
}

fn unsupported_authored_shader_message(
    shader_ref: &str,
    effect_package_root: Option<&Path>,
    lookup: &SceneShaderSourceLookup,
) -> String {
    let sources = lookup
        .matched_paths
        .iter()
        .map(|path| path.display().to_string())
        .collect::<Vec<_>>()
        .join(", ");
    if let Some(reason) = phase10b_blocked_effect_reason(shader_ref) {
        if let Some(effect_package_root) = effect_package_root {
            return format!(
                "shader {shader_ref} resolved authored source assets ({sources}) from effect package {} or Scene roots, but phase-10b does not support that authored shader family as single-pass compat. {reason}",
                effect_package_root.display()
            );
        }
        return format!(
            "shader {shader_ref} resolved authored source assets ({sources}) from Scene roots, but phase-10b does not support that authored shader family as single-pass compat. {reason}"
        );
    }

    if let Some(effect_package_root) = effect_package_root {
        format!(
            "shader {shader_ref} resolved authored source assets ({sources}) from effect package {} or Scene roots, but phase-10b does not support that authored shader family as explicit single-pass compat",
            effect_package_root.display()
        )
    } else {
        format!(
            "shader {shader_ref} resolved authored source assets ({sources}) from Scene roots, but phase-10b does not support that authored shader family as explicit single-pass compat"
        )
    }
}

fn resolve_supported_authored_effect_program(
    resolver: &SceneResourceResolver,
    shader_ref: &str,
    combos: &BTreeMap<String, i32>,
) -> Result<Option<SceneShaderProgram>, String> {
    let Some(contract) = phase10b_supported_effect_contract_for_shader_ref(shader_ref) else {
        return Ok(None);
    };
    let asset_path = "assets/shaders/compat/scene-effect-compat.metal";
    let metal_source_path = resolver
        .resolve_relative_path(asset_path)
        .ok_or_else(|| format!("built-in Scene shader asset {asset_path} could not be resolved"))?;
    Ok(Some(SceneShaderProgram {
        key: format!("effect-compat:{:?}:{shader_ref}", contract.kind),
        kind: SceneShaderProgramKind::EffectCompat(contract.kind),
        metal_source_path,
        vertex_entry: "phase10_effect_vertex",
        fragment_entry: "phase10_effect_fragment",
        variant_defines: compat_effect_shader_defines(contract.kind, combos),
    }))
}

pub fn phase10b_supported_effect_contract_for_shader_ref(
    shader_ref: &str,
) -> Option<&'static ScenePhase10bEffectContract> {
    let normalized = normalized_shader_stem(shader_ref);
    match normalized.as_str() {
        "pulse" => Some(&PULSE_CONTRACT),
        "shake" => Some(&SHAKE_CONTRACT),
        "waterripple" => Some(&WATERRIPPLE_CONTRACT),
        "waterwaves" => Some(&WATERWAVES_CONTRACT),
        "tint" => Some(&TINT_CONTRACT),
        "scroll" => Some(&SCROLL_CONTRACT),
        "lightshafts" => Some(&LIGHTSHAFTS_CONTRACT),
        "foliagesway" => Some(&FOLIAGESWAY_CONTRACT),
        "circle" => Some(&CIRCLE_CONTRACT),
        "opacity" => Some(&OPACITY_CONTRACT),
        "transform" => Some(&TRANSFORM_CONTRACT),
        "skew" => Some(&SKEW_CONTRACT),
        "perspective" => Some(&PERSPECTIVE_CONTRACT),
        "spin" => Some(&SPIN_CONTRACT),
        "swing" => Some(&SWING_CONTRACT),
        "twirl" => Some(&TWIRL_CONTRACT),
        "chromaticaberration" => Some(&CHROMATIC_ABERRATION_CONTRACT),
        "colorkey" => Some(&COLORKEY_CONTRACT),
        "fisheye" => Some(&FISHEYE_CONTRACT),
        "edgedetection" => Some(&EDGEDETECTION_CONTRACT),
        "iris" => Some(&IRIS_CONTRACT),
        "cloudmotion" => Some(&CLOUDMOTION_CONTRACT),
        "clouds" => Some(&CLOUDS_CONTRACT),
        "waterflow" => Some(&WATERFLOW_CONTRACT),
        "nitro" => Some(&NITRO_CONTRACT),
        "blend" => Some(&BLEND_CONTRACT),
        "reflection" => Some(&REFLECTION_CONTRACT),
        "shimmer" => Some(&SHIMMER_CONTRACT),
        "filmgrain" => Some(&FILMGRAIN_CONTRACT),
        "vhs" => Some(&VHS_CONTRACT),
        "blendgradient" => Some(&BLENDGRADIENT_CONTRACT),
        "caustics" => Some(&WATERCAUSTICS_CONTRACT),
        "watercaustics" => Some(&WATERCAUSTICS_CONTRACT),
        "xray" => Some(&XRAY_CONTRACT),
        _ => None,
    }
}

pub fn phase10b_effect_contract_for_kind(
    kind: SceneCompatEffectKind,
) -> Option<&'static ScenePhase10bEffectContract> {
    match kind {
        SceneCompatEffectKind::Pulse => Some(&PULSE_CONTRACT),
        SceneCompatEffectKind::Shake => Some(&SHAKE_CONTRACT),
        SceneCompatEffectKind::WaterRipple => Some(&WATERRIPPLE_CONTRACT),
        SceneCompatEffectKind::WaterWaves => Some(&WATERWAVES_CONTRACT),
        SceneCompatEffectKind::Tint => Some(&TINT_CONTRACT),
        SceneCompatEffectKind::Scroll => Some(&SCROLL_CONTRACT),
        SceneCompatEffectKind::LightShafts => Some(&LIGHTSHAFTS_CONTRACT),
        SceneCompatEffectKind::FoliageSway => Some(&FOLIAGESWAY_CONTRACT),
        SceneCompatEffectKind::Circle => Some(&CIRCLE_CONTRACT),
        SceneCompatEffectKind::Opacity => Some(&OPACITY_CONTRACT),
        SceneCompatEffectKind::Transform => Some(&TRANSFORM_CONTRACT),
        SceneCompatEffectKind::Skew => Some(&SKEW_CONTRACT),
        SceneCompatEffectKind::Perspective => Some(&PERSPECTIVE_CONTRACT),
        SceneCompatEffectKind::Spin => Some(&SPIN_CONTRACT),
        SceneCompatEffectKind::Swing => Some(&SWING_CONTRACT),
        SceneCompatEffectKind::Twirl => Some(&TWIRL_CONTRACT),
        SceneCompatEffectKind::ChromaticAberration => Some(&CHROMATIC_ABERRATION_CONTRACT),
        SceneCompatEffectKind::ColorKey => Some(&COLORKEY_CONTRACT),
        SceneCompatEffectKind::FishEye => Some(&FISHEYE_CONTRACT),
        SceneCompatEffectKind::EdgeDetection => Some(&EDGEDETECTION_CONTRACT),
        SceneCompatEffectKind::Iris => Some(&IRIS_CONTRACT),
        SceneCompatEffectKind::CloudMotion => Some(&CLOUDMOTION_CONTRACT),
        SceneCompatEffectKind::Clouds => Some(&CLOUDS_CONTRACT),
        SceneCompatEffectKind::WaterFlow => Some(&WATERFLOW_CONTRACT),
        SceneCompatEffectKind::Nitro => Some(&NITRO_CONTRACT),
        SceneCompatEffectKind::Blend => Some(&BLEND_CONTRACT),
        SceneCompatEffectKind::Reflection => Some(&REFLECTION_CONTRACT),
        SceneCompatEffectKind::Shimmer => Some(&SHIMMER_CONTRACT),
        SceneCompatEffectKind::FilmGrain => Some(&FILMGRAIN_CONTRACT),
        SceneCompatEffectKind::Vhs => Some(&VHS_CONTRACT),
        SceneCompatEffectKind::BlendGradient => Some(&BLENDGRADIENT_CONTRACT),
        SceneCompatEffectKind::WaterCaustics => Some(&WATERCAUSTICS_CONTRACT),
        SceneCompatEffectKind::XRay => Some(&XRAY_CONTRACT),
    }
}

pub fn phase10b_blocked_effect_reason(shader_ref: &str) -> Option<&'static str> {
    match normalized_shader_stem(shader_ref).as_str() {
        "blur" => Some(
            "blur requires phase-10d named render targets, multi-pass order, previous-texture chaining, and copy-background lifecycle support.",
        ),
        "shine" => Some(
            "shine requires phase-10d named render targets, multi-pass order, previous-texture chaining, and copy-background lifecycle support.",
        ),
        _ => None,
    }
}

fn normalized_shader_stem(shader_ref: &str) -> String {
    let candidate = Path::new(shader_ref)
        .file_stem()
        .or_else(|| Path::new(shader_ref).file_name())
        .and_then(|value| value.to_str())
        .unwrap_or(shader_ref);
    candidate
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

fn shader_program_vertex_entry(kind: SceneShaderProgramKind) -> &'static str {
    match kind {
        SceneShaderProgramKind::Sprite => "compat_sprite_vertex",
        SceneShaderProgramKind::Model => "compat_model_vertex",
        SceneShaderProgramKind::MaskAlpha => "compat_mask_vertex",
        SceneShaderProgramKind::MaskApply => "compat_mask_apply_vertex",
        SceneShaderProgramKind::Copy => "compat_copy_vertex",
        SceneShaderProgramKind::EffectCompat(_) => "phase10_effect_vertex",
    }
}

fn shader_program_fragment_entry(kind: SceneShaderProgramKind) -> &'static str {
    match kind {
        SceneShaderProgramKind::Sprite => "compat_sprite_fragment",
        SceneShaderProgramKind::Model => "compat_model_fragment",
        SceneShaderProgramKind::MaskAlpha => "compat_mask_alpha_fragment",
        SceneShaderProgramKind::MaskApply => "compat_mask_apply_fragment",
        SceneShaderProgramKind::Copy => "compat_copy_fragment",
        SceneShaderProgramKind::EffectCompat(_) => "phase10_effect_fragment",
    }
}

fn shader_program_base_defines(kind: SceneShaderProgramKind) -> BTreeMap<String, i32> {
    match kind {
        SceneShaderProgramKind::EffectCompat(kind) => {
            compat_effect_shader_defines(kind, &BTreeMap::new())
        }
        _ => BTreeMap::new(),
    }
}

fn compat_effect_shader_defines(
    kind: SceneCompatEffectKind,
    combos: &BTreeMap<String, i32>,
) -> BTreeMap<String, i32> {
    let mut defines = BTreeMap::new();
    match kind {
        SceneCompatEffectKind::Pulse => {
            defines.insert("PHASE10_EFFECT_PULSE".to_string(), 1);
        }
        SceneCompatEffectKind::Shake => {
            defines.insert("PHASE10_EFFECT_SHAKE".to_string(), 1);
        }
        SceneCompatEffectKind::WaterRipple => {
            defines.insert("PHASE10_EFFECT_WATERRIPPLE".to_string(), 1);
        }
        SceneCompatEffectKind::WaterWaves => {
            defines.insert("PHASE10_EFFECT_WATERWAVES".to_string(), 1);
        }
        SceneCompatEffectKind::Tint => {
            defines.insert("PHASE10_EFFECT_TINT".to_string(), 1);
        }
        SceneCompatEffectKind::Scroll => {
            defines.insert("PHASE10_EFFECT_SCROLL".to_string(), 1);
        }
        SceneCompatEffectKind::LightShafts => {
            defines.insert("PHASE10_EFFECT_LIGHTSHAFTS".to_string(), 1);
        }
        SceneCompatEffectKind::FoliageSway => {
            defines.insert("PHASE10_EFFECT_FOLIAGESWAY".to_string(), 1);
        }
        SceneCompatEffectKind::Circle => {
            defines.insert("PHASE10_EFFECT_CIRCLE".to_string(), 1);
        }
        SceneCompatEffectKind::Opacity => {
            defines.insert("PHASE10_EFFECT_OPACITY".to_string(), 1);
        }
        SceneCompatEffectKind::Transform => {
            defines.insert("PHASE10_EFFECT_TRANSFORM".to_string(), 1);
        }
        SceneCompatEffectKind::Skew => {
            defines.insert("PHASE10_EFFECT_SKEW".to_string(), 1);
        }
        SceneCompatEffectKind::Perspective => {
            defines.insert("PHASE10_EFFECT_PERSPECTIVE".to_string(), 1);
        }
        SceneCompatEffectKind::Spin => {
            defines.insert("PHASE10_EFFECT_SPIN".to_string(), 1);
        }
        SceneCompatEffectKind::Swing => {
            defines.insert("PHASE10_EFFECT_SWING".to_string(), 1);
        }
        SceneCompatEffectKind::Twirl => {
            defines.insert("PHASE10_EFFECT_TWIRL".to_string(), 1);
        }
        SceneCompatEffectKind::ChromaticAberration => {
            defines.insert("PHASE10_EFFECT_CHROMATIC_ABERRATION".to_string(), 1);
        }
        SceneCompatEffectKind::ColorKey => {
            defines.insert("PHASE10_EFFECT_COLORKEY".to_string(), 1);
        }
        SceneCompatEffectKind::FishEye => {
            defines.insert("PHASE10_EFFECT_FISHEYE".to_string(), 1);
        }
        SceneCompatEffectKind::EdgeDetection => {
            defines.insert("PHASE10_EFFECT_EDGEDETECTION".to_string(), 1);
        }
        SceneCompatEffectKind::Iris => {
            defines.insert("PHASE10_EFFECT_IRIS".to_string(), 1);
        }
        SceneCompatEffectKind::CloudMotion => {
            defines.insert("PHASE10_EFFECT_CLOUDMOTION".to_string(), 1);
        }
        SceneCompatEffectKind::Clouds => {
            defines.insert("PHASE10_EFFECT_CLOUDS".to_string(), 1);
        }
        SceneCompatEffectKind::WaterFlow => {
            defines.insert("PHASE10_EFFECT_WATERFLOW".to_string(), 1);
        }
        SceneCompatEffectKind::Nitro => {
            defines.insert("PHASE10_EFFECT_NITRO".to_string(), 1);
        }
        SceneCompatEffectKind::Blend => {
            defines.insert("PHASE10_EFFECT_BLEND".to_string(), 1);
        }
        SceneCompatEffectKind::Reflection => {
            defines.insert("PHASE10_EFFECT_REFLECTION".to_string(), 1);
        }
        SceneCompatEffectKind::Shimmer => {
            defines.insert("PHASE10_EFFECT_SHIMMER".to_string(), 1);
        }
        SceneCompatEffectKind::FilmGrain => {
            defines.insert("PHASE10_EFFECT_FILMGRAIN".to_string(), 1);
        }
        SceneCompatEffectKind::Vhs => {
            defines.insert("PHASE10_EFFECT_VHS".to_string(), 1);
        }
        SceneCompatEffectKind::BlendGradient => {
            defines.insert("PHASE10_EFFECT_BLENDGRADIENT".to_string(), 1);
        }
        SceneCompatEffectKind::WaterCaustics => {
            defines.insert("PHASE10_EFFECT_WATERCAUSTICS".to_string(), 1);
        }
        SceneCompatEffectKind::XRay => {
            defines.insert("PHASE10_EFFECT_XRAY".to_string(), 1);
        }
    }
    if let Some(contract) = phase10b_effect_contract_for_kind(kind) {
        for (name, value) in contract.supported_combo_defaults {
            defines.insert((*name).to_string(), *value);
        }
    }
    for (name, value) in combos {
        defines.insert(name.clone(), *value);
    }
    defines
}

fn resolve_texture_candidates_for_material(
    resolver: &SceneResourceResolver,
    material_path: &str,
    resolved_material_path: &Path,
    effect_package_root: Option<&Path>,
    texture_name: &str,
) -> Vec<PathBuf> {
    if let Some(effect_package_root) = effect_package_root {
        resolver.resolve_texture_candidates_with_local_root(
            Some(material_path),
            Some(resolved_material_path),
            texture_name,
            SceneResourceRootKind::EffectPackage,
            effect_package_root,
        )
    } else {
        resolver
            .inspect_texture_candidates(
                Some(material_path),
                Some(resolved_material_path),
                texture_name,
            )
            .matched_paths
    }
}

pub fn shader_ref_requires_phase10_graph_semantics(shader_ref: &str) -> bool {
    let shader_ref = shader_ref.trim();
    if shader_ref.is_empty() {
        return false;
    }

    let lower = shader_ref.to_ascii_lowercase();
    if lower.contains("clippingmaskimage4")
        || lower.contains("copy")
        || lower.contains("model")
        || lower.contains("puppet")
    {
        return true;
    }
    if lower.contains("genericimage")
        || lower.contains("image")
        || lower.contains("sprite")
        || lower.contains("default")
        || lower.contains("textured")
    {
        return false;
    }

    true
}

pub fn parse_combo_map(value: Option<&Value>) -> BTreeMap<String, i32> {
    let mut combos = BTreeMap::new();
    let Some(value) = value.and_then(Value::as_object) else {
        return combos;
    };
    for (key, value) in value {
        let parsed = value
            .as_i64()
            .map(|value| value as i32)
            .or_else(|| value.as_bool().map(|value| if value { 1 } else { 0 }))
            .or_else(|| value.as_str().and_then(|text| text.parse::<i32>().ok()));
        if let Some(parsed) = parsed {
            combos.insert(key.to_string(), parsed);
        }
    }
    combos
}

pub fn parse_uniform_map_from_constants(
    constants: &BTreeMap<String, Value>,
) -> BTreeMap<String, SceneMaterialUniformValue> {
    constants
        .iter()
        .filter_map(|(name, value)| parse_uniform_value(value).map(|parsed| (name.clone(), parsed)))
        .collect()
}

fn parse_texture_list(value: Option<&Value>) -> Vec<Option<String>> {
    value
        .and_then(Value::as_array)
        .map(|textures| {
            textures
                .iter()
                .map(|texture| texture.as_str().map(ToString::to_string))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

fn parse_material_uniform_map(
    pass: &Value,
    material_json: &Value,
) -> BTreeMap<String, SceneMaterialUniformValue> {
    let mut uniforms = BTreeMap::new();
    merge_uniform_scope(&mut uniforms, material_json);
    merge_uniform_scope(&mut uniforms, pass);
    uniforms
}

fn merge_uniform_scope(target: &mut BTreeMap<String, SceneMaterialUniformValue>, scope: &Value) {
    for key in [
        "defaults",
        "defaultvalues",
        "uniforms",
        "constants",
        "constantshadervalues",
    ] {
        if let Some(entries) = scope.get(key).and_then(Value::as_object) {
            for (name, value) in entries {
                if let Some(parsed) = parse_uniform_value(value) {
                    target.insert(name.clone(), parsed);
                }
            }
        }
    }

    let Some(entries) = scope.as_object() else {
        return;
    };
    for (name, value) in entries {
        if is_reserved_material_field(name) {
            continue;
        }
        if let Some(parsed) = parse_uniform_value(value) {
            target.insert(name.clone(), parsed);
        }
    }
}

fn is_reserved_material_field(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "shader"
            | "textures"
            | "usertextures"
            | "passes"
            | "effects"
            | "file"
            | "bind"
            | "combos"
            | "blending"
            | "constants"
            | "constantshadervalues"
            | "uniforms"
            | "defaults"
            | "defaultvalues"
            | "target"
            | "targetscale"
            | "targetformat"
    )
}

fn parse_uniform_value(value: &Value) -> Option<SceneMaterialUniformValue> {
    if let Some(number) = value.as_f64() {
        return SceneMaterialUniformValue::from_floats(&[number as f32]);
    }
    if let Some(boolean) = value.as_bool() {
        return SceneMaterialUniformValue::from_floats(&[if boolean { 1.0 } else { 0.0 }]);
    }
    if let Some(text) = value.as_str() {
        let parts = text
            .split(|character: char| character == ' ' || character == ',' || character == '\t')
            .filter(|part| !part.is_empty())
            .filter_map(|part| part.parse::<f32>().ok())
            .collect::<Vec<_>>();
        return SceneMaterialUniformValue::from_floats(&parts);
    }
    if let Some(array) = value.as_array() {
        let parts = array
            .iter()
            .filter_map(|item| {
                item.as_f64()
                    .map(|value| value as f32)
                    .or_else(|| item.as_bool().map(|value| if value { 1.0 } else { 0.0 }))
            })
            .collect::<Vec<_>>();
        return SceneMaterialUniformValue::from_floats(&parts);
    }
    if let Some(value) = value.get("value") {
        return parse_uniform_value(value);
    }
    None
}

fn material_pass_values(json: &Value) -> Vec<Value> {
    if let Some(passes) = json.get("passes").and_then(Value::as_array) {
        return passes.clone();
    }
    if material_declares_inline_pass(json) {
        return vec![json.clone()];
    }
    Vec::new()
}

fn material_declares_inline_pass(json: &Value) -> bool {
    json.get("shader")
        .and_then(Value::as_str)
        .map(|shader| !shader.trim().is_empty())
        .unwrap_or(false)
        || json
            .get("textures")
            .and_then(Value::as_array)
            .map(|textures| !textures.is_empty())
            .unwrap_or(false)
        || json
            .get("usertextures")
            .and_then(Value::as_array)
            .map(|textures| !textures.is_empty())
            .unwrap_or(false)
        || json.get("blending").and_then(Value::as_str).is_some()
        || json
            .get("combos")
            .and_then(Value::as_object)
            .map(|combos| !combos.is_empty())
            .unwrap_or(false)
}

fn parse_material_blend_mode(value: Option<&str>) -> SceneRenderBlendMode {
    match value.unwrap_or_default().to_ascii_lowercase().as_str() {
        "additive" | "add" => SceneRenderBlendMode::Additive,
        "multiply" | "mul" => SceneRenderBlendMode::Multiply,
        _ => SceneRenderBlendMode::Normal,
    }
}

fn effect_paths(value: &Value) -> Vec<String> {
    value
        .get("effects")
        .and_then(Value::as_array)
        .map(|effects| {
            effects
                .iter()
                .filter_map(|effect| {
                    effect
                        .get("file")
                        .and_then(Value::as_str)
                        .or_else(|| effect.as_str())
                })
                .map(ToString::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

fn read_json(path: &Path) -> Result<Value, String> {
    let raw = fs::read_to_string(path)
        .map_err(|error| format!("unable to read {}: {error}", path.display()))?;
    serde_json::from_str(&raw)
        .map_err(|error| format!("unable to parse {}: {error}", path.display()))
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, fs};

    use tempfile::tempdir;

    use crate::services::scene_resource_service::{SceneResourceResolver, SceneResourceRootKind};

    use super::{
        inspect_scene_material_summary, load_scene_effect_plan, load_scene_material_plan,
        load_scene_material_plan_with_effect_package_root, preprocess_scene_shader_source,
        resolve_shader_program, resolve_shader_program_with_effect_package_root,
        shader_ref_requires_phase10_graph_semantics, SceneCompatEffectKind, SceneShaderProgramKind,
    };

    fn write(path: &std::path::Path, body: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("parent dir");
        }
        fs::write(path, body).expect("write fixture");
    }

    #[test]
    fn material_plan_resolves_builtin_compat_program_and_textures() {
        let temp = tempdir().expect("temp dir");
        let managed = temp.path().join("managed");
        let extracted = managed.join("extracted");
        let builtin = temp.path().join("builtin");
        write(
            &builtin.join("assets/shaders/compat/scene-sprite.metal"),
            "fragment float4 compat_fragment() { return float4(1); }",
        );
        write(
            &extracted.join("materials/hero.material"),
            r#"{
              "passes":[
                {
                  "shader":"genericimage4",
                  "textures":["textures/hero"],
                  "combos":{"NORMALMAP":true}
                }
              ]
            }"#,
        );
        write(&extracted.join("textures/hero.tex"), "fake");
        let resolver =
            SceneResourceResolver::for_managed_root_with_builtin_root(&managed, &builtin);

        let plan =
            load_scene_material_plan(&resolver, "materials/hero.material").expect("material plan");

        assert_eq!(plan.passes.len(), 1);
        assert_eq!(plan.passes[0].program.kind, SceneShaderProgramKind::Sprite);
        assert_eq!(plan.passes[0].textures[0].slot_name, "g_Texture0");
        assert!(plan.passes[0].textures[0].resolved_path.is_some());
        assert_eq!(plan.passes[0].combos.get("NORMALMAP"), Some(&1));
    }

    #[test]
    fn effect_plan_reads_pass_bindings_and_dependencies() {
        let temp = tempdir().expect("temp dir");
        let managed = temp.path().join("managed");
        let extracted = managed.join("extracted");
        let builtin = temp.path().join("builtin");
        write(
            &extracted.join("effects/blur.effect"),
            r#"{
              "version": 2,
              "copybackground": true,
              "dependencies": ["shaders/fx/blur.frag", "shaders/fx/blur.vert"],
              "passes": [
                {
                  "material": "materials/fx.material",
                  "copyBackground": true,
                  "bind": [{"name":"input","index":0}]
                }
              ]
            }"#,
        );
        let resolver =
            SceneResourceResolver::for_managed_root_with_builtin_root(&managed, &builtin);

        let plan = load_scene_effect_plan(&resolver, "effects/blur.effect").expect("effect plan");

        assert_eq!(plan.version, Some(2));
        assert!(plan.copy_background);
        assert_eq!(plan.shader_dependencies.len(), 2);
        assert!(plan.passes[0].copy_background);
        assert_eq!(plan.passes[0].bindings[0].name, "input");
        assert_eq!(
            plan.passes[0].material_path.as_deref(),
            Some("materials/fx.material")
        );
    }

    #[test]
    fn effect_plan_resolves_pass_material_relative_to_package_root() {
        let temp = tempdir().expect("temp dir");
        let managed = temp.path().join("managed");
        let extracted = managed.join("extracted");
        let builtin = temp.path().join("builtin");
        write(
            &builtin.join("assets/shaders/compat/scene-sprite.metal"),
            "fragment float4 compat_fragment() { return float4(1); }",
        );
        write(
            &extracted.join("effects/waterripple/effect.json"),
            r#"{"passes":[{"material":"materials/effects/waterripple.json","bind":[{"name":"input","index":0}]}]}"#,
        );
        write(
            &extracted.join("effects/waterripple/materials/effects/waterripple.json"),
            r#"{"passes":[{"shader":"genericimage4","textures":["textures/ripple"]}]}"#,
        );
        write(
            &extracted.join("effects/waterripple/textures/ripple.tex"),
            "fake",
        );
        let resolver =
            SceneResourceResolver::for_managed_root_with_builtin_root(&managed, &builtin);

        let plan =
            load_scene_effect_plan(&resolver, "effects/waterripple/effect.json").expect("effect");
        let material_lookup = plan.passes[0]
            .material_lookup
            .as_ref()
            .expect("material lookup");
        assert_eq!(
            material_lookup.matched_path.as_deref(),
            Some(
                extracted
                    .join("effects/waterripple/materials/effects/waterripple.json")
                    .as_path()
            )
        );
        assert_eq!(
            material_lookup.matched_root_kind,
            Some(SceneResourceRootKind::ExtractedContent)
        );

        let material = load_scene_material_plan_with_effect_package_root(
            &resolver,
            "materials/effects/waterripple.json",
            &plan.effect_package_root,
        )
        .expect("effect material");
        assert_eq!(material.passes.len(), 1);
        assert!(material.passes[0].textures[0].resolved_path.is_some());
    }

    #[test]
    fn effect_plan_resolves_texture_dependencies_with_tex_sidecar_candidates() {
        let temp = tempdir().expect("temp dir");
        let managed = temp.path().join("managed");
        let extracted = managed.join("extracted");
        let builtin = temp.path().join("builtin");
        let effect_root = extracted.join("effects/waterripple");
        write(
            &effect_root.join("effect.json"),
            r#"{
              "dependencies": [
                "materials/effects/waterripple.json",
                "materials/effects/waterripplenormal.png",
                "materials/effects/waterripplenormal.tex-json",
                "shaders/effects/waterripple.frag",
                "shaders/effects/waterripple.vert"
              ],
              "passes":[{"material":"materials/effects/waterripple.json"}]
            }"#,
        );
        write(
            &effect_root.join("materials/effects/waterripple.json"),
            r#"{"passes":[{"shader":"genericimage4","textures":["effects/waterripplenormal"]}]}"#,
        );
        write(
            &effect_root.join("materials/effects/waterripplenormal.tex"),
            "tex",
        );
        write(
            &effect_root.join("shaders/effects/waterripple.frag"),
            "frag",
        );
        write(
            &effect_root.join("shaders/effects/waterripple.vert"),
            "vert",
        );

        let resolver =
            SceneResourceResolver::for_managed_root_with_builtin_root(&managed, &builtin);
        let plan =
            load_scene_effect_plan(&resolver, "effects/waterripple/effect.json").expect("effect");

        assert!(plan
            .dependency_lookups
            .iter()
            .all(|lookup| lookup.matched_path.is_some()));
        assert!(plan
            .dependency_lookups
            .iter()
            .filter(|lookup| lookup.authored_reference.ends_with(".png"))
            .any(|lookup| lookup
                .matched_path
                .as_ref()
                .is_some_and(|path| path.ends_with("waterripplenormal.tex"))));
        assert!(plan
            .dependency_lookups
            .iter()
            .filter(|lookup| lookup.authored_reference.ends_with(".tex-json"))
            .any(|lookup| lookup
                .matched_path
                .as_ref()
                .is_some_and(|path| path.ends_with("waterripplenormal.tex"))));
    }

    #[test]
    fn effect_plan_resolves_shader_dependencies_and_programs_from_package_root() {
        let temp = tempdir().expect("temp dir");
        let managed = temp.path().join("managed");
        let extracted = managed.join("extracted");
        let builtin = temp.path().join("builtin");
        write(
            &extracted.join("effects/pulse/effect.json"),
            r#"{
              "dependencies":["shaders/effects/pulse.metal"],
              "passes":[{"material":"materials/effects/pulse.json"}]
            }"#,
        );
        write(
            &extracted.join("effects/pulse/shaders/effects/pulse.metal"),
            "fragment float4 pulse_fragment() { return float4(1); }",
        );
        write(
            &extracted.join("effects/pulse/materials/effects/pulse.json"),
            r#"{"passes":[{"shader":"shaders/effects/pulse.metal"}]}"#,
        );
        let resolver =
            SceneResourceResolver::for_managed_root_with_builtin_root(&managed, &builtin);

        let plan = load_scene_effect_plan(&resolver, "effects/pulse/effect.json").expect("effect");
        assert_eq!(
            plan.dependency_lookups[0].matched_path.as_deref(),
            Some(
                extracted
                    .join("effects/pulse/shaders/effects/pulse.metal")
                    .as_path()
            )
        );
        assert_eq!(
            plan.dependency_lookups[0].matched_root_kind,
            Some(SceneResourceRootKind::ExtractedContent)
        );

        let program = resolve_shader_program_with_effect_package_root(
            &resolver,
            "shaders/effects/pulse.metal",
            &BTreeMap::new(),
            &plan.effect_package_root,
        )
        .expect("effect shader");
        assert_eq!(
            program.metal_source_path,
            extracted.join("effects/pulse/shaders/effects/pulse.metal")
        );

        let material = load_scene_material_plan_with_effect_package_root(
            &resolver,
            "materials/effects/pulse.json",
            &plan.effect_package_root,
        )
        .expect("effect material");
        assert_eq!(
            material.passes[0].program.metal_source_path,
            extracted.join("effects/pulse/shaders/effects/pulse.metal")
        );
    }

    #[test]
    fn effect_plan_resolves_package_materials_from_external_assets_root() {
        let temp = tempdir().expect("temp dir");
        let managed = temp.path().join("managed");
        let builtin = temp.path().join("builtin");
        let external = temp.path().join("external-assets");
        write(
            &builtin.join("assets/shaders/compat/scene-sprite.metal"),
            "fragment float4 compat_fragment() { return float4(1); }",
        );
        write(
            &external.join("effects/shake/effect.json"),
            r#"{"passes":[{"material":"materials/effects/shake.json"}]}"#,
        );
        write(
            &external.join("effects/shake/materials/effects/shake.json"),
            r#"{"passes":[{"shader":"genericimage4"}]}"#,
        );
        let resolver = SceneResourceResolver::for_managed_root_with_asset_roots(
            &managed,
            &builtin,
            Some(external.clone()),
        );

        let plan = load_scene_effect_plan(&resolver, "effects/shake/effect.json").expect("effect");
        assert_eq!(plan.effect_path, external.join("effects/shake/effect.json"));
        assert_eq!(
            plan.passes[0]
                .material_lookup
                .as_ref()
                .and_then(|lookup| lookup.matched_root_kind),
            Some(SceneResourceRootKind::ExternalAssets)
        );
        let material = load_scene_material_plan_with_effect_package_root(
            &resolver,
            "materials/effects/shake.json",
            &plan.effect_package_root,
        )
        .expect("effect material");
        assert_eq!(
            material.material_path,
            external.join("effects/shake/materials/effects/shake.json")
        );
    }

    #[test]
    fn effect_plan_resolves_authored_shader_pair_dependencies_from_package_root() {
        let temp = tempdir().expect("temp dir");
        let managed = temp.path().join("managed");
        let extracted = managed.join("extracted");
        let builtin = temp.path().join("builtin");
        write(
            &extracted.join("effects/pulse/effect.json"),
            r#"{
              "dependencies":["effects/pulse-dependency"],
              "passes":[]
            }"#,
        );
        write(
            &extracted.join("effects/pulse/shaders/effects/pulse-dependency.vert"),
            "void main() {}",
        );
        write(
            &extracted.join("effects/pulse/shaders/effects/pulse-dependency.frag"),
            "void main() {}",
        );
        let resolver =
            SceneResourceResolver::for_managed_root_with_builtin_root(&managed, &builtin);

        let plan = load_scene_effect_plan(&resolver, "effects/pulse/effect.json").expect("effect");
        assert_eq!(plan.shader_dependencies, vec!["effects/pulse-dependency"]);
        assert_eq!(
            plan.dependency_lookups[0].matched_path.as_deref(),
            Some(
                extracted
                    .join("effects/pulse/shaders/effects/pulse-dependency.vert")
                    .as_path()
            )
        );
        assert_eq!(
            plan.dependency_lookups[0].matched_root_kind,
            Some(SceneResourceRootKind::ExtractedContent)
        );
    }

    #[test]
    fn supported_authored_effect_shader_pairs_map_to_phase_10b_compat_program() {
        let temp = tempdir().expect("temp dir");
        let managed = temp.path().join("managed");
        let extracted = managed.join("extracted");
        let builtin = temp.path().join("builtin");
        let external = temp.path().join("external-assets");
        write(
            &external.join("effects/pulse/materials/effects/pulse.json"),
            r#"{"passes":[{"shader":"effects/pulse"}]}"#,
        );
        write(
            &external.join("effects/pulse/shaders/effects/pulse.vert"),
            "void main() {}",
        );
        write(
            &external.join("effects/pulse/shaders/effects/pulse.frag"),
            "void main() {}",
        );
        write(
            &builtin.join("assets/shaders/compat/scene-effect-compat.metal"),
            "fragment float4 phase10_effect_fragment() { return float4(1); }",
        );
        let resolver = SceneResourceResolver::for_managed_root_with_asset_roots(
            &managed,
            &builtin,
            Some(external.clone()),
        );

        let plan = load_scene_material_plan_with_effect_package_root(
            &resolver,
            "materials/effects/pulse.json",
            &extracted.join("effects/pulse"),
        )
        .expect("pulse compat effect material");

        assert_eq!(plan.passes.len(), 1);
        assert_eq!(
            plan.passes[0].program.kind,
            SceneShaderProgramKind::EffectCompat(SceneCompatEffectKind::Pulse)
        );
        assert_eq!(plan.passes[0].program.vertex_entry, "phase10_effect_vertex");
        assert_eq!(
            plan.passes[0].program.fragment_entry,
            "phase10_effect_fragment"
        );
    }

    #[test]
    fn batch1_authored_effect_shader_pairs_map_to_phase_10b_compat_program() {
        let temp = tempdir().expect("temp dir");
        let managed = temp.path().join("managed");
        let builtin = temp.path().join("builtin");
        let external = temp.path().join("external-assets");
        write(
            &builtin.join("assets/shaders/compat/scene-effect-compat.metal"),
            "fragment float4 phase10_effect_fragment() { return float4(1); }",
        );
        let resolver = SceneResourceResolver::for_managed_root_with_asset_roots(
            &managed,
            &builtin,
            Some(external.clone()),
        );

        for (family, expected) in [
            ("lightshafts", SceneCompatEffectKind::LightShafts),
            ("foliagesway", SceneCompatEffectKind::FoliageSway),
            ("circle", SceneCompatEffectKind::Circle),
        ] {
            write(
                &external.join(format!("effects/{family}/materials/effects/{family}.json")),
                format!(r#"{{"passes":[{{"shader":"effects/{family}"}}]}}"#).as_str(),
            );
            write(
                &external.join(format!("effects/{family}/shaders/effects/{family}.vert")),
                "void main() {}",
            );
            write(
                &external.join(format!("effects/{family}/shaders/effects/{family}.frag")),
                "void main() {}",
            );

            let plan = load_scene_material_plan_with_effect_package_root(
                &resolver,
                &format!("materials/effects/{family}.json"),
                &external.join(format!("effects/{family}")),
            )
            .expect("batch1 compat effect material");

            assert_eq!(
                plan.passes[0].program.kind,
                SceneShaderProgramKind::EffectCompat(expected)
            );
        }
    }

    #[test]
    fn batch2_group_a_authored_effect_shader_pairs_map_to_phase_10b_compat_program() {
        let temp = tempdir().expect("temp dir");
        let managed = temp.path().join("managed");
        let builtin = temp.path().join("builtin");
        let external = temp.path().join("external-assets");
        write(
            &builtin.join("assets/shaders/compat/scene-effect-compat.metal"),
            "fragment float4 phase10_effect_fragment() { return float4(1); }",
        );
        let resolver = SceneResourceResolver::for_managed_root_with_asset_roots(
            &managed,
            &builtin,
            Some(external.clone()),
        );

        for (family, expected) in [
            ("opacity", SceneCompatEffectKind::Opacity),
            ("transform", SceneCompatEffectKind::Transform),
            ("skew", SceneCompatEffectKind::Skew),
            ("perspective", SceneCompatEffectKind::Perspective),
            ("spin", SceneCompatEffectKind::Spin),
        ] {
            write(
                &external.join(format!("effects/{family}/materials/effects/{family}.json")),
                format!(r#"{{"passes":[{{"shader":"effects/{family}"}}]}}"#).as_str(),
            );
            write(
                &external.join(format!("effects/{family}/shaders/effects/{family}.vert")),
                "void main() {}",
            );
            write(
                &external.join(format!("effects/{family}/shaders/effects/{family}.frag")),
                "void main() {}",
            );

            let plan = load_scene_material_plan_with_effect_package_root(
                &resolver,
                &format!("materials/effects/{family}.json"),
                &external.join(format!("effects/{family}")),
            )
            .expect("batch2 group a compat effect material");

            assert_eq!(
                plan.passes[0].program.kind,
                SceneShaderProgramKind::EffectCompat(expected)
            );
        }
    }

    #[test]
    fn batch2_group_b_authored_effect_shader_pairs_map_to_phase_10b_compat_program() {
        let temp = tempdir().expect("temp dir");
        let managed = temp.path().join("managed");
        let builtin = temp.path().join("builtin");
        let external = temp.path().join("external-assets");
        write(
            &builtin.join("assets/shaders/compat/scene-effect-compat.metal"),
            "fragment float4 phase10_effect_fragment() { return float4(1); }",
        );
        let resolver = SceneResourceResolver::for_managed_root_with_asset_roots(
            &managed,
            &builtin,
            Some(external.clone()),
        );

        for (family, shader_ref, expected) in [
            (
                "chromaticaberration",
                "effects/chromatic_aberration",
                SceneCompatEffectKind::ChromaticAberration,
            ),
            ("colorkey", "effects/colorkey", SceneCompatEffectKind::ColorKey),
            ("fisheye", "effects/fisheye", SceneCompatEffectKind::FishEye),
            (
                "edgedetection",
                "effects/edgedetection",
                SceneCompatEffectKind::EdgeDetection,
            ),
        ] {
            write(
                &external.join(format!("effects/{family}/materials/effects/{family}.json")),
                format!(r#"{{"passes":[{{"shader":"{shader_ref}"}}]}}"#).as_str(),
            );
            let shader_stem = shader_ref.rsplit('/').next().expect("shader stem");
            write(
                &external.join(format!("effects/{family}/shaders/effects/{shader_stem}.vert")),
                "void main() {}",
            );
            write(
                &external.join(format!("effects/{family}/shaders/effects/{shader_stem}.frag")),
                "void main() {}",
            );

            let plan = load_scene_material_plan_with_effect_package_root(
                &resolver,
                &format!("materials/effects/{family}.json"),
                &external.join(format!("effects/{family}")),
            )
            .expect("batch2 group b compat effect material");

            assert_eq!(
                plan.passes[0].program.kind,
                SceneShaderProgramKind::EffectCompat(expected)
            );
        }
    }

    #[test]
    fn batch2_group_c_authored_effect_shader_pairs_map_to_phase_10b_compat_program() {
        let temp = tempdir().expect("temp dir");
        let managed = temp.path().join("managed");
        let builtin = temp.path().join("builtin");
        let external = temp.path().join("external-assets");
        write(
            &builtin.join("assets/shaders/compat/scene-effect-compat.metal"),
            "fragment float4 phase10_effect_fragment() { return float4(1); }",
        );
        let resolver = SceneResourceResolver::for_managed_root_with_asset_roots(
            &managed,
            &builtin,
            Some(external.clone()),
        );

        for (family, shader_ref, expected) in [
            ("cloudmotion", "effects/cloudmotion", SceneCompatEffectKind::CloudMotion),
            ("clouds", "effects/clouds", SceneCompatEffectKind::Clouds),
            ("waterflow", "effects/waterflow", SceneCompatEffectKind::WaterFlow),
            ("nitro", "effects/nitro", SceneCompatEffectKind::Nitro),
        ] {
            write(
                &external.join(format!("effects/{family}/materials/effects/{family}.json")),
                format!(r#"{{"passes":[{{"shader":"{shader_ref}"}}]}}"#).as_str(),
            );
            let shader_stem = shader_ref.rsplit('/').next().expect("shader stem");
            write(
                &external.join(format!("effects/{family}/shaders/effects/{shader_stem}.vert")),
                "void main() {}",
            );
            write(
                &external.join(format!("effects/{family}/shaders/effects/{shader_stem}.frag")),
                "void main() {}",
            );

            let plan = load_scene_material_plan_with_effect_package_root(
                &resolver,
                &format!("materials/effects/{family}.json"),
                &external.join(format!("effects/{family}")),
            )
            .expect("batch2 group c compat effect material");

            assert_eq!(
                plan.passes[0].program.kind,
                SceneShaderProgramKind::EffectCompat(expected)
            );
        }
    }

    #[test]
    fn material_plan_preserves_sparse_authored_texture_slot_ordinals() {
        let temp = tempdir().expect("temp dir");
        let managed = temp.path().join("managed");
        let extracted = managed.join("extracted");
        let builtin = temp.path().join("builtin");
        write(
            &builtin.join("assets/shaders/compat/scene-sprite.metal"),
            "fragment float4 compat_sprite_fragment() { return float4(1); }",
        );
        write(
            &extracted.join("materials/layer.material"),
            r#"{"passes":[{"shader":"genericimage4","textures":[null,null,"textures/mask.png"]}]}"#,
        );
        write(&extracted.join("textures/mask.png"), "png");
        let resolver =
            SceneResourceResolver::for_managed_root_with_builtin_root(&managed, &builtin);

        let plan =
            load_scene_material_plan(&resolver, "materials/layer.material").expect("material");

        assert_eq!(plan.passes.len(), 1);
        assert_eq!(plan.passes[0].textures.len(), 3);
        assert_eq!(plan.passes[0].textures[0].slot_index, 0);
        assert_eq!(plan.passes[0].textures[0].texture_name, None);
        assert_eq!(plan.passes[0].textures[1].slot_index, 1);
        assert_eq!(plan.passes[0].textures[1].texture_name, None);
        assert_eq!(plan.passes[0].textures[2].slot_index, 2);
        assert_eq!(
            plan.passes[0].textures[2].texture_name.as_deref(),
            Some("textures/mask.png")
        );
    }

    #[test]
    fn phase10b_supported_families_expose_explicit_contract_defaults_and_slots() {
        let families = [
            (SceneCompatEffectKind::Shake, vec![0, 1, 2, 3]),
            (SceneCompatEffectKind::Pulse, vec![0, 1, 2]),
            (SceneCompatEffectKind::WaterRipple, vec![0, 1, 2]),
            (SceneCompatEffectKind::WaterWaves, vec![0, 1, 2]),
            (SceneCompatEffectKind::Tint, vec![0, 1]),
            (SceneCompatEffectKind::Scroll, vec![0]),
            (SceneCompatEffectKind::LightShafts, vec![0, 1, 2]),
            (SceneCompatEffectKind::FoliageSway, vec![0, 1, 2]),
            (SceneCompatEffectKind::Circle, vec![0]),
            (SceneCompatEffectKind::Opacity, vec![0, 1]),
            (SceneCompatEffectKind::Transform, vec![0]),
            (SceneCompatEffectKind::Skew, vec![0]),
            (SceneCompatEffectKind::Perspective, vec![0]),
            (SceneCompatEffectKind::Spin, vec![0, 1]),
            (SceneCompatEffectKind::Swing, vec![0, 1, 2]),
            (SceneCompatEffectKind::Twirl, vec![0, 1, 2]),
            (SceneCompatEffectKind::ChromaticAberration, vec![0, 1]),
            (SceneCompatEffectKind::ColorKey, vec![0]),
            (SceneCompatEffectKind::FishEye, vec![0]),
            (SceneCompatEffectKind::EdgeDetection, vec![0]),
            (SceneCompatEffectKind::Iris, vec![0, 1]),
            (SceneCompatEffectKind::CloudMotion, vec![0, 1, 2]),
            (SceneCompatEffectKind::Clouds, vec![0, 1, 2]),
            (SceneCompatEffectKind::WaterFlow, vec![0, 1, 2]),
            (SceneCompatEffectKind::Nitro, vec![0, 1, 2]),
            (SceneCompatEffectKind::Blend, vec![0, 1, 7]),
            (SceneCompatEffectKind::Reflection, vec![0, 1]),
            (SceneCompatEffectKind::Shimmer, vec![0, 1, 2, 3]),
            (SceneCompatEffectKind::FilmGrain, vec![0, 1, 2]),
            (SceneCompatEffectKind::Vhs, vec![0, 1, 2]),
            (SceneCompatEffectKind::BlendGradient, vec![0, 1, 2, 3]),
            (SceneCompatEffectKind::WaterCaustics, vec![0, 1, 2, 3, 4, 5]),
            (SceneCompatEffectKind::XRay, vec![0, 1, 2, 3]),
        ];

        for (kind, supported_slots) in families {
            let contract =
                super::phase10b_effect_contract_for_kind(kind).expect("phase-10b contract");
            assert_eq!(contract.kind, kind);
            assert_eq!(contract.supported_texture_slots, supported_slots.as_slice());
            assert_eq!(
                contract
                    .runtime_binding_layout
                    .iter()
                    .map(|slot| slot.slot)
                    .collect::<Vec<_>>(),
                supported_slots
            );
        }

        let pulse = super::phase10b_effect_contract_for_kind(SceneCompatEffectKind::Pulse)
            .expect("pulse contract");
        assert!(pulse.supported_combo_defaults.contains(&("BLENDMODE", 9)));
        assert!(pulse.supported_uniforms.contains(&"noiseamount"));
        assert_eq!(
            pulse
                .runtime_binding_layout
                .iter()
                .map(|slot| slot.uv_space)
                .collect::<Vec<_>>(),
            vec![
                super::ScenePhase10bUvSpace::PrimaryInput,
                super::ScenePhase10bUvSpace::AuxTexture,
                super::ScenePhase10bUvSpace::MaskTexture,
            ]
        );

        let scroll = super::phase10b_effect_contract_for_kind(SceneCompatEffectKind::Scroll)
            .expect("scroll contract");
        assert_eq!(scroll.supported_combo_defaults, &[]);
        assert!(scroll.supported_uniforms.contains(&"repeat"));

        let shake = super::phase10b_effect_contract_for_kind(SceneCompatEffectKind::Shake)
            .expect("shake contract");
        assert_eq!(
            shake
                .runtime_binding_layout
                .iter()
                .map(|slot| slot.uv_space)
                .collect::<Vec<_>>(),
            vec![
                super::ScenePhase10bUvSpace::PrimaryInput,
                super::ScenePhase10bUvSpace::AuxTexture,
                super::ScenePhase10bUvSpace::AuxTexture,
                super::ScenePhase10bUvSpace::MaskTexture,
            ]
        );

        let lightshafts =
            super::phase10b_effect_contract_for_kind(SceneCompatEffectKind::LightShafts)
                .expect("lightshafts contract");
        assert!(lightshafts.supported_uniforms.contains(&"point0"));
        assert!(lightshafts.supported_uniforms.contains(&"rayspeed"));
        assert!(lightshafts.supported_combo_defaults.contains(&("RAYMODE", 0)));

        let foliage =
            super::phase10b_effect_contract_for_kind(SceneCompatEffectKind::FoliageSway)
                .expect("foliage contract");
        assert!(foliage.supported_uniforms.contains(&"strength"));
        assert!(foliage.supported_combo_defaults.contains(&("MODE", 0)));

        let circle = super::phase10b_effect_contract_for_kind(SceneCompatEffectKind::Circle)
            .expect("circle contract");
        assert_eq!(circle.supported_texture_slots, &[0]);
        assert!(circle.supported_uniforms.is_empty());

        let opacity = super::phase10b_effect_contract_for_kind(SceneCompatEffectKind::Opacity)
            .expect("opacity contract");
        assert!(opacity.supported_combo_defaults.contains(&("MASK", 0)));
        assert!(opacity.supported_uniforms.contains(&"alpha"));
        assert!(opacity.supported_uniforms.contains(&"useralpha"));

        let transform =
            super::phase10b_effect_contract_for_kind(SceneCompatEffectKind::Transform)
                .expect("transform contract");
        assert!(transform.supported_combo_defaults.contains(&("CLAMP", 1)));
        assert!(transform.supported_uniforms.is_empty());

        let skew = super::phase10b_effect_contract_for_kind(SceneCompatEffectKind::Skew)
            .expect("skew contract");
        assert!(skew.supported_combo_defaults.contains(&("REPEAT", 1)));

        let perspective =
            super::phase10b_effect_contract_for_kind(SceneCompatEffectKind::Perspective)
                .expect("perspective contract");
        assert!(perspective.supported_combo_defaults.contains(&("REPEAT", 0)));
        assert!(perspective.supported_uniforms.contains(&"point0"));
        assert!(perspective.supported_uniforms.contains(&"point3"));

        let spin = super::phase10b_effect_contract_for_kind(SceneCompatEffectKind::Spin)
            .expect("spin contract");
        assert!(spin.supported_combo_defaults.contains(&("MASK", 0)));
        assert!(spin.supported_combo_defaults.contains(&("REPEAT", 1)));
        assert!(spin.supported_uniforms.contains(&"center"));
        assert!(spin.supported_uniforms.contains(&"spincenter"));

        let swing = super::phase10b_effect_contract_for_kind(SceneCompatEffectKind::Swing)
            .expect("swing contract");
        assert!(swing.supported_combo_defaults.contains(&("DOUBLESIDED", 0)));
        assert!(swing.supported_combo_defaults.contains(&("NOISE", 0)));
        assert!(swing.supported_uniforms.contains(&"point0"));
        assert!(swing.supported_uniforms.contains(&"point1"));

        let twirl = super::phase10b_effect_contract_for_kind(SceneCompatEffectKind::Twirl)
            .expect("twirl contract");
        assert!(twirl.supported_combo_defaults.contains(&("ELLIPTICAL", 1)));
        assert!(twirl.supported_combo_defaults.contains(&("INNER", 0)));
        assert!(twirl.supported_uniforms.contains(&"ratio"));
        assert!(twirl.supported_uniforms.contains(&"angle"));

        let chromatic = super::phase10b_effect_contract_for_kind(
            SceneCompatEffectKind::ChromaticAberration,
        )
        .expect("chromatic contract");
        assert!(chromatic.supported_combo_defaults.contains(&("MODE", 0)));
        assert!(chromatic.supported_combo_defaults.contains(&("VARIATION", 0)));
        assert!(chromatic
            .supported_uniforms
            .contains(&"uieditorpropertiesstrength"));

        let colorkey = super::phase10b_effect_contract_for_kind(SceneCompatEffectKind::ColorKey)
            .expect("colorkey contract");
        assert!(colorkey.supported_combo_defaults.contains(&("INVERT", 0)));
        assert!(colorkey.supported_combo_defaults.contains(&("FLATTEN", 0)));
        assert!(colorkey.supported_uniforms.contains(&"fuzziness"));

        let fisheye = super::phase10b_effect_contract_for_kind(SceneCompatEffectKind::FishEye)
            .expect("fisheye contract");
        assert!(fisheye.supported_combo_defaults.contains(&("BACKGROUND", 1)));
        assert!(fisheye.supported_uniforms.contains(&"distortion"));

        let edgedetection = super::phase10b_effect_contract_for_kind(
            SceneCompatEffectKind::EdgeDetection,
        )
        .expect("edgedetection contract");
        assert!(edgedetection
            .supported_combo_defaults
            .contains(&("BLENDMODE", 0)));
        assert!(edgedetection.supported_uniforms.contains(&"outlinecolor"));

        let iris = super::phase10b_effect_contract_for_kind(SceneCompatEffectKind::Iris)
            .expect("iris contract");
        assert!(iris.supported_combo_defaults.contains(&("BACKGROUND", 0)));
        assert!(iris.supported_uniforms.contains(&"scale"));

        let cloudmotion = super::phase10b_effect_contract_for_kind(SceneCompatEffectKind::CloudMotion)
            .expect("cloudmotion contract");
        assert!(cloudmotion.supported_combo_defaults.contains(&("MASK", 0)));
        assert!(cloudmotion.supported_uniforms.contains(&"amount"));

        let clouds = super::phase10b_effect_contract_for_kind(SceneCompatEffectKind::Clouds)
            .expect("clouds contract");
        assert!(clouds.supported_combo_defaults.contains(&("SHADING", 7)));
        assert!(clouds.supported_uniforms.contains(&"colorstart"));

        let waterflow = super::phase10b_effect_contract_for_kind(SceneCompatEffectKind::WaterFlow)
            .expect("waterflow contract");
        assert_eq!(waterflow.supported_combo_defaults, &[]);
        assert!(waterflow.supported_uniforms.contains(&"phasescale"));

        let nitro = super::phase10b_effect_contract_for_kind(SceneCompatEffectKind::Nitro)
            .expect("nitro contract");
        assert!(nitro.supported_combo_defaults.contains(&("BLENDMODE", 22)));
        assert!(nitro.supported_uniforms.contains(&"multiply"));

        let blend = super::phase10b_effect_contract_for_kind(SceneCompatEffectKind::Blend)
            .expect("blend contract");
        assert!(blend.supported_combo_defaults.contains(&("NUMBLENDTEXTURES", 1)));
        assert!(blend.supported_combo_defaults.contains(&("OPACITYMASK", 0)));
        assert!(blend.supported_uniforms.contains(&"multiply"));

        let watercaustics =
            super::phase10b_effect_contract_for_kind(SceneCompatEffectKind::WaterCaustics)
                .expect("watercaustics contract");
        assert!(watercaustics.supported_combo_defaults.contains(&("MODE", 0)));
        assert!(watercaustics
            .supported_uniforms
            .contains(&"uieditorpropertiesbrightness"));

        let reflection = super::phase10b_effect_contract_for_kind(SceneCompatEffectKind::Reflection)
            .expect("reflection contract");
        assert!(reflection.supported_combo_defaults.contains(&("PERSPECTIVE", 0)));
        assert!(reflection.supported_uniforms.contains(&"alpha"));

        let shimmer = super::phase10b_effect_contract_for_kind(SceneCompatEffectKind::Shimmer)
            .expect("shimmer contract");
        assert!(shimmer.supported_combo_defaults.contains(&("MODE", 0)));
        assert!(shimmer.supported_combo_defaults.contains(&("OFFSET", 0)));
        assert!(shimmer
            .supported_uniforms
            .contains(&"uieditorpropertiesbrightness"));
        assert_eq!(
            shimmer
                .runtime_binding_layout
                .iter()
                .map(|slot| slot.uv_space)
                .collect::<Vec<_>>(),
            vec![
                super::ScenePhase10bUvSpace::PrimaryInput,
                super::ScenePhase10bUvSpace::MaskTexture,
                super::ScenePhase10bUvSpace::AuxTexture,
                super::ScenePhase10bUvSpace::AuxTexture,
            ]
        );

        let filmgrain = super::phase10b_effect_contract_for_kind(SceneCompatEffectKind::FilmGrain)
            .expect("filmgrain contract");
        assert!(filmgrain.supported_combo_defaults.contains(&("GREYSCALE", 1)));
        assert!(filmgrain
            .supported_uniforms
            .contains(&"uieditorpropertiesstrength"));

        let vhs = super::phase10b_effect_contract_for_kind(SceneCompatEffectKind::Vhs)
            .expect("vhs contract");
        assert!(vhs
            .supported_combo_defaults
            .contains(&("INVERTARTIFACTS", 1)));
        assert!(vhs.supported_uniforms.contains(&"tracking"));

        let blendgradient =
            super::phase10b_effect_contract_for_kind(SceneCompatEffectKind::BlendGradient)
                .expect("blendgradient contract");
        assert!(blendgradient
            .supported_combo_defaults
            .contains(&("TRANSFORMUV", 0)));
        assert!(blendgradient.supported_uniforms.contains(&"gradientscale"));

        let xray = super::phase10b_effect_contract_for_kind(SceneCompatEffectKind::XRay)
            .expect("xray contract");
        assert!(xray.supported_combo_defaults.contains(&("OPACITYMASK", 0)));
        assert!(xray
            .supported_uniforms
            .contains(&"uieditorpropertiesmultiply"));
        assert_eq!(
            xray
                .runtime_binding_layout
                .iter()
                .map(|slot| slot.semantic)
                .collect::<Vec<_>>(),
            vec![
                super::ScenePhase10bBindingSemantic::PreviousInput,
                super::ScenePhase10bBindingSemantic::GradientTexture,
                super::ScenePhase10bBindingSemantic::SpriteTexture,
                super::ScenePhase10bBindingSemantic::OpacityMask,
            ]
        );
    }

    #[test]
    fn phase10b_blur_and_shine_report_phase10d_blockers() {
        let blur_reason =
            super::phase10b_blocked_effect_reason("effects/blur").expect("blur blocker");
        let shine_reason =
            super::phase10b_blocked_effect_reason("effects/shine").expect("shine blocker");

        assert!(blur_reason.contains("phase-10d"));
        assert!(blur_reason.contains("named render targets"));
        assert!(shine_reason.contains("phase-10d"));
        assert!(shine_reason.contains("copy-background lifecycle"));
    }

    #[test]
    fn tint_compat_effect_uses_authored_blendmode_default_without_overriding_combos() {
        let defaults =
            super::compat_effect_shader_defines(SceneCompatEffectKind::Tint, &BTreeMap::new());
        assert_eq!(defaults.get("BLENDMODE"), Some(&30));

        let explicit = super::compat_effect_shader_defines(
            SceneCompatEffectKind::Tint,
            &BTreeMap::from([("BLENDMODE".to_string(), 2)]),
        );
        assert_eq!(explicit.get("BLENDMODE"), Some(&2));
    }

    #[test]
    fn unknown_authored_effect_shader_pairs_remain_phase_10b_unsupported_not_missing() {
        let temp = tempdir().expect("temp dir");
        let managed = temp.path().join("managed");
        let extracted = managed.join("extracted");
        let builtin = temp.path().join("builtin");
        let external = temp.path().join("external-assets");
        write(
            &external.join("effects/mystery/materials/effects/mystery.json"),
            r#"{"passes":[{"shader":"effects/mystery"}]}"#,
        );
        write(
            &external.join("effects/mystery/shaders/effects/mystery.vert"),
            "void main() {}",
        );
        write(
            &external.join("effects/mystery/shaders/effects/mystery.frag"),
            "void main() {}",
        );
        let resolver = SceneResourceResolver::for_managed_root_with_asset_roots(
            &managed,
            &builtin,
            Some(external.clone()),
        );

        let error = load_scene_material_plan_with_effect_package_root(
            &resolver,
            "materials/effects/mystery.json",
            &extracted.join("effects/mystery"),
        )
        .expect_err("unknown authored effect shader should remain unsupported in phase-10b");

        assert!(error.contains("resolved authored source assets"));
        assert!(error.contains(
            "phase-10b does not support that authored shader family as explicit single-pass compat"
        ));
        assert!(!error.contains("could not be resolved"));
    }

    #[test]
    fn shader_preprocessor_comments_include_and_injects_defines() {
        let mut defines = BTreeMap::new();
        defines.insert("CLIPPINGTARGET".to_string(), 1);
        let source = r#"#include "lib/common.glsl"
uniform sampler2D g_Texture0 {"label":"base"}
float main()；"#;

        let processed = preprocess_scene_shader_source(source, &defines);

        assert!(processed.contains("#define CLIPPINGTARGET 1"));
        assert!(processed.contains("// #include \"lib/common.glsl\""));
        assert!(!processed.contains("{\"label\":\"base\"}"));
        assert!(!processed.contains('；'));
    }

    #[test]
    fn resolve_shader_program_maps_clipping_combo_to_mask_apply() {
        let temp = tempdir().expect("temp dir");
        let managed = temp.path().join("managed");
        let builtin = temp.path().join("builtin");
        write(
            &builtin.join("assets/shaders/compat/scene-mask-apply.metal"),
            "fragment float4 mask_apply_fragment() { return float4(1); }",
        );
        let resolver =
            SceneResourceResolver::for_managed_root_with_builtin_root(&managed, &builtin);
        let program = resolve_shader_program(
            &resolver,
            "genericimage4",
            &BTreeMap::from([("CLIPPINGTARGET".to_string(), 1)]),
        )
        .expect("program");

        assert_eq!(program.kind, SceneShaderProgramKind::MaskApply);
    }

    #[test]
    fn material_plan_accepts_inline_single_pass_shape() {
        let temp = tempdir().expect("temp dir");
        let managed = temp.path().join("managed");
        let extracted = managed.join("extracted");
        let builtin = temp.path().join("builtin");
        write(
            &builtin.join("assets/shaders/compat/scene-sprite.metal"),
            "fragment float4 compat_fragment() { return float4(1); }",
        );
        write(
            &extracted.join("materials/inline.material"),
            r#"{
              "shader":"genericimage4",
              "textures":["textures/hero"]
            }"#,
        );
        let resolver =
            SceneResourceResolver::for_managed_root_with_builtin_root(&managed, &builtin);

        let plan = load_scene_material_plan(&resolver, "materials/inline.material")
            .expect("inline material plan");

        assert_eq!(plan.passes.len(), 1);
        assert_eq!(plan.passes[0].program.kind, SceneShaderProgramKind::Sprite);
        assert_eq!(plan.passes[0].textures.len(), 1);
    }

    #[test]
    fn material_summary_marks_simple_sprite_inline_material_as_baseline() {
        let temp = tempdir().expect("temp dir");
        let managed = temp.path().join("managed");
        let extracted = managed.join("extracted");
        let builtin = temp.path().join("builtin");
        write(
            &extracted.join("materials/inline.material"),
            r#"{
              "shader":"genericimage4",
              "textures":["textures/hero"]
            }"#,
        );
        let resolver =
            SceneResourceResolver::for_managed_root_with_builtin_root(&managed, &builtin);

        let summary = inspect_scene_material_summary(&resolver, "materials/inline.material")
            .expect("material summary");

        assert_eq!(summary.pass_count, 1);
        assert_eq!(summary.max_texture_count, 1);
        assert!(!summary.requires_phase10_graph);
    }

    #[test]
    fn shader_semantics_keep_genericimage_on_baseline_and_model_on_phase10() {
        assert!(!shader_ref_requires_phase10_graph_semantics(
            "genericimage4"
        ));
        assert!(shader_ref_requires_phase10_graph_semantics("modelimage"));
        assert!(shader_ref_requires_phase10_graph_semantics(
            "shaders/custom.frag"
        ));
    }
}
