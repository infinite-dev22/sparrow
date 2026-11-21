//! Dart and Flutter workflow panels and workspace notification state.

use floem::reactive::{RwSignal, SignalUpdate};

mod notifications;
mod pull_requests;
mod running_devices;

pub use notifications::notifications;
pub use pull_requests::pull_requests;
pub use running_devices::running_devices;

#[derive(Clone, Debug)]
pub struct Notice {
    pub id: u64,
    pub resolution_id: Option<u64>,
    pub time: String,
    pub title: String,
    pub body: String,
}
pub fn record(notices: RwSignal<Vec<Notice>>, title: &str, body: &str) {
    notices.update(|items| {
        let id = items.last().map_or(1, |n| n.id + 1);
        items.push(Notice {
            id,
            resolution_id: None,
            time: chrono::Local::now().format("%H:%M").to_string(),
            title: title.into(),
            body: body.into(),
        });
        if items.len() > 200 {
            items.remove(0);
        }
    });
}
/// One completion per process, even if automatic and manual requests joined it.
pub fn record_pub_get_result(notices:RwSignal<Vec<Notice>>,id:u64,result:Result<(),String>){
    notices.update(|items|{
        if items.iter().any(|notice|notice.resolution_id==Some(id)){return;}
        let next=items.last().map_or(1,|n|n.id+1);
        items.push(Notice{id:next,resolution_id:Some(id),time:chrono::Local::now().format("%H:%M").to_string(),title:"Pub Get".into(),body:match result{Ok(())=>"Package dependencies resolved.".into(),Err(error)=>error}});
        if items.len()>200 {items.remove(0);}
    });
}
