use libp2p::kad::QueryResult;

fn test() {
    match QueryResult::GetProviders(Ok(Default::default())) {
        QueryResult::GetProviders(Ok(result)) => {
            // This will show us what fields are available
            let _ = result;
        }
        _ => {}
    }
}
