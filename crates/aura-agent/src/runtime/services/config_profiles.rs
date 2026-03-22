macro_rules! impl_service_config_profiles {
    ($type:ty { $($body:item)* }) => {
        impl $type {
            $($body)*
        }
    };
}

pub(crate) use impl_service_config_profiles;
