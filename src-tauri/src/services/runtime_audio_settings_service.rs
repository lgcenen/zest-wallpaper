#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeAudioOutputDevice {
    pub id: String,
    pub name: String,
    pub is_system_default: bool,
}

pub fn normalize_output_volume_percent(volume: f64) -> f64 {
    if !volume.is_finite() {
        return 1.0;
    }
    (volume / 100.0).clamp(0.0, 1.0)
}

pub fn effective_output_volume(source_volume: f64, output_volume: f64) -> f64 {
    source_volume.clamp(0.0, 1.0) * output_volume.clamp(0.0, 1.0)
}

pub fn normalize_output_device_uid(uid: Option<String>) -> Option<String> {
    uid.and_then(|value| {
        let trimmed = value.trim();
        if trimmed.is_empty() || trimmed == "system-default" {
            None
        } else {
            Some(trimmed.to_string())
        }
    })
}

pub fn list_audio_output_devices() -> Vec<RuntimeAudioOutputDevice> {
    let mut devices = vec![RuntimeAudioOutputDevice {
        id: "system-default".to_string(),
        name: "System Default Output".to_string(),
        is_system_default: true,
    }];

    #[cfg(target_os = "macos")]
    {
        devices.extend(macos_audio_output_devices());
    }

    devices
}

#[cfg(target_os = "macos")]
fn macos_audio_output_devices() -> Vec<RuntimeAudioOutputDevice> {
    use std::{ffi::c_void, mem, ptr::NonNull};

    use objc2_core_audio::{
        kAudioDevicePropertyDeviceUID, kAudioDevicePropertyStreamConfiguration,
        kAudioHardwarePropertyDevices, kAudioObjectPropertyElementMain, kAudioObjectPropertyName,
        kAudioObjectPropertyScopeGlobal, kAudioObjectPropertyScopeOutput, kAudioObjectSystemObject,
        AudioObjectGetPropertyData, AudioObjectGetPropertyDataSize, AudioObjectID,
        AudioObjectPropertyAddress,
    };
    use objc2_core_audio_types::{AudioBuffer, AudioBufferList};
    use objc2_core_foundation::{CFRetained, CFString};

    fn property_address(selector: u32, scope: u32) -> AudioObjectPropertyAddress {
        AudioObjectPropertyAddress {
            mSelector: selector,
            mScope: scope,
            mElement: kAudioObjectPropertyElementMain,
        }
    }

    unsafe fn property_data_size(
        object_id: AudioObjectID,
        address: &mut AudioObjectPropertyAddress,
    ) -> Option<u32> {
        let mut size = 0_u32;
        let status = AudioObjectGetPropertyDataSize(
            object_id,
            NonNull::from(address),
            0,
            std::ptr::null(),
            NonNull::from(&mut size),
        );
        (status == 0 && size > 0).then_some(size)
    }

    unsafe fn cf_string_property(object_id: AudioObjectID, selector: u32) -> Option<String> {
        let mut address = property_address(selector, kAudioObjectPropertyScopeGlobal);
        let mut raw: *const CFString = std::ptr::null();
        let mut size = mem::size_of::<*const CFString>() as u32;
        let status = AudioObjectGetPropertyData(
            object_id,
            NonNull::from(&mut address),
            0,
            std::ptr::null(),
            NonNull::from(&mut size),
            NonNull::new(&mut raw as *mut _ as *mut c_void)?,
        );
        if status != 0 || raw.is_null() {
            return None;
        }
        let retained = CFRetained::from_raw(NonNull::new(raw as *mut CFString)?);
        Some(retained.to_string())
    }

    unsafe fn has_output_streams(device_id: AudioObjectID) -> bool {
        let mut address = property_address(
            kAudioDevicePropertyStreamConfiguration,
            kAudioObjectPropertyScopeOutput,
        );
        let Some(mut size) = property_data_size(device_id, &mut address) else {
            return false;
        };
        let mut storage = vec![0_u8; size as usize];
        let Some(storage_ptr) = NonNull::new(storage.as_mut_ptr() as *mut c_void) else {
            return false;
        };
        let status = AudioObjectGetPropertyData(
            device_id,
            NonNull::from(&mut address),
            0,
            std::ptr::null(),
            NonNull::from(&mut size),
            storage_ptr,
        );
        if status != 0 || size < mem::size_of::<AudioBufferList>() as u32 {
            return false;
        }

        let list = storage.as_ptr() as *const AudioBufferList;
        let buffer_count = (*list).mNumberBuffers as usize;
        if buffer_count == 0 {
            return false;
        }
        let buffers = std::slice::from_raw_parts(
            std::ptr::addr_of!((*list).mBuffers) as *const AudioBuffer,
            buffer_count,
        );
        buffers.iter().any(|buffer| buffer.mNumberChannels > 0)
    }

    unsafe {
        let mut address = property_address(
            kAudioHardwarePropertyDevices,
            kAudioObjectPropertyScopeGlobal,
        );
        let Some(mut size) =
            property_data_size(kAudioObjectSystemObject as AudioObjectID, &mut address)
        else {
            return Vec::new();
        };
        let count = size as usize / mem::size_of::<AudioObjectID>();
        if count == 0 {
            return Vec::new();
        }

        let mut ids = vec![0 as AudioObjectID; count];
        let Some(ids_ptr) = NonNull::new(ids.as_mut_ptr() as *mut c_void) else {
            return Vec::new();
        };
        let status = AudioObjectGetPropertyData(
            kAudioObjectSystemObject as AudioObjectID,
            NonNull::from(&mut address),
            0,
            std::ptr::null(),
            NonNull::from(&mut size),
            ids_ptr,
        );
        if status != 0 {
            return Vec::new();
        }

        ids.into_iter()
            .filter(|id| has_output_streams(*id))
            .filter_map(|id| {
                let uid = cf_string_property(id, kAudioDevicePropertyDeviceUID)?;
                let name =
                    cf_string_property(id, kAudioObjectPropertyName).unwrap_or_else(|| uid.clone());
                Some(RuntimeAudioOutputDevice {
                    id: uid,
                    name,
                    is_system_default: false,
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::{
        effective_output_volume, normalize_output_device_uid, normalize_output_volume_percent,
    };

    #[test]
    fn output_volume_percent_normalizes_and_scales_source_volume() {
        assert_eq!(normalize_output_volume_percent(100.0), 1.0);
        assert_eq!(normalize_output_volume_percent(35.0), 0.35);
        assert_eq!(normalize_output_volume_percent(-20.0), 0.0);
        assert_eq!(normalize_output_volume_percent(250.0), 1.0);
        assert_eq!(normalize_output_volume_percent(f64::NAN), 1.0);

        assert!((effective_output_volume(0.8, 0.5) - 0.4).abs() < f64::EPSILON);
        assert_eq!(effective_output_volume(1.2, 0.25), 0.25);
        assert_eq!(effective_output_volume(0.75, -1.0), 0.0);
    }

    #[test]
    fn output_device_uid_normalizes_system_default_and_custom_ids() {
        assert_eq!(normalize_output_device_uid(None), None);
        assert_eq!(normalize_output_device_uid(Some("".to_string())), None);
        assert_eq!(
            normalize_output_device_uid(Some("system-default".to_string())),
            None
        );
        assert_eq!(
            normalize_output_device_uid(Some("  BuiltInSpeakerDevice  ".to_string())),
            Some("BuiltInSpeakerDevice".to_string())
        );
    }
}
