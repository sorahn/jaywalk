//! Version policy shared by the build script and its headless regression tests.

#[derive(Debug, PartialEq, Eq)]
pub struct Identity {
    pub numeric: String,
    pub display: String,
}

pub fn resolve(
    release: Option<&str>,
    release_profile: bool,
    provenance: &str,
) -> Result<Identity, &'static str> {
    let Some(release) = release else {
        return Ok(Identity {
            numeric: "0.0.0".into(),
            display: format!("dev ({provenance})"),
        });
    };
    let parts = release.split('.').collect::<Vec<_>>();
    if parts.len() != 3 {
        return Err("JAYWALK_RELEASE_VERSION must use year.month.release-number");
    }
    let mut numbers = [0_u32; 3];
    for (index, part) in parts.iter().enumerate() {
        if part.is_empty() || !part.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err("JAYWALK_RELEASE_VERSION must contain three numeric components");
        }
        numbers[index] = part
            .parse()
            .map_err(|_| "JAYWALK_RELEASE_VERSION component is too large")?;
        if numbers[index].to_string() != *part {
            return Err("JAYWALK_RELEASE_VERSION must not contain leading zeroes");
        }
    }
    if !(2000..=9999).contains(&numbers[0]) || !(1..=12).contains(&numbers[1]) || numbers[2] == 0 {
        return Err("JAYWALK_RELEASE_VERSION has an invalid year, month or release number");
    }
    Ok(Identity {
        numeric: release.into(),
        display: if release_profile {
            release.into()
        } else {
            format!("{release}-dev ({provenance})")
        },
    })
}
