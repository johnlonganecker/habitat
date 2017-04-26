// Copyright (c) 2016-2017 Chef Software Inc. and/or applicable contributors
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! A collection of handlers for the Scheduler dispatcher

use time::PreciseTime;
use hab_net::server::Envelope;
use protocol::net::{self, ErrCode};
use protocol::scheduler as proto;
use protobuf::RepeatedField;
use zmq;

use super::ServerState;
use error::Result;

pub fn group_create(req: &mut Envelope,
                    sock: &mut zmq::Socket,
                    state: &mut ServerState)
                    -> Result<()> {
    let msg: proto::GroupCreate = try!(req.parse_msg());
    println!("group_create message: {:?}", msg);

    let project_name = format!("{}/{}", msg.get_origin(), msg.get_package());
    let mut projects = Vec::new();

    // Get the ident for the root package
    let mut start_time;
    let mut end_time;

    let project_ident = {
        let graph = state.graph().read().unwrap();
        start_time = PreciseTime::now();
        let ret = match graph.resolve(&project_name) {
            Some(s) => s,
            None => {
                error!("GroupCreate, project ident not found");
                let err = net::err(ErrCode::ENTITY_NOT_FOUND, "sc:group-create:1");
                try!(req.reply_complete(sock, &err));
                return Ok(());
            }
        };
        end_time = PreciseTime::now();
        ret
    };
    println!("Resolved project name: {} sec\n", start_time.to(end_time));

    // Add the root package if needed
    if !msg.get_deps_only() {
        projects.push((project_name.clone(), project_ident.clone()));
    }

    // Search the packages graph to find the reverse dependencies
    let rdeps_opt = {
        let graph = state.graph().read().unwrap();
        start_time = PreciseTime::now();
        let ret = graph.rdeps(&project_ident);
        end_time = PreciseTime::now();
        ret
    };

    match rdeps_opt {
        Some(rdeps) => {
            println!("Graph rdeps: {} items ({} sec)\n",
                     rdeps.len(),
                     start_time.to(end_time));

            for s in rdeps {
                println!("Adding to projects: {} ({})", s.0, s.1);
                projects.push(s);
            }
        }
        None => {
            println!("Graph rdeps: no entries found");
        }
    }

    let group = if projects.is_empty() {
        println!("No projects need building - group is complete");
        let mut new_group = proto::Group::new();
        let projects = RepeatedField::new();
        new_group.set_id(0);
        new_group.set_state(proto::GroupState::Complete);
        new_group.set_projects(projects);
        new_group
    } else {
        let new_group = state.datastore().create_group(&msg, projects)?;
        try!(state.schedule_cli().notify_work());
        new_group
    };

    try!(req.reply_complete(sock, &group));
    Ok(())
}

pub fn group_get(req: &mut Envelope,
                 sock: &mut zmq::Socket,
                 state: &mut ServerState)
                 -> Result<()> {
    let msg: proto::GroupGet = try!(req.parse_msg());
    println!("group_get message: {:?}", msg);

    let group_opt = match state.datastore().get_group(&msg) {
        Ok(group_opt) => group_opt,
        Err(err) => {
            error!("Unable to retrieve group {}, err: {:?}",
                   msg.get_group_id(),
                   err);
            None
        }
    };

    match group_opt {
        Some(group) => {
            try!(req.reply_complete(sock, &group));
        }
        None => {
            let err = net::err(ErrCode::ENTITY_NOT_FOUND, "sc:schedule-get:1");
            try!(req.reply_complete(sock, &err));
        }
    }

    Ok(())
}

pub fn package_create(req: &mut Envelope,
                      sock: &mut zmq::Socket,
                      state: &mut ServerState)
                      -> Result<()> {
    let msg: proto::PackageCreate = try!(req.parse_msg());
    println!("package_create message: {:?}", msg);

    let package = state.datastore().create_package(&msg)?;

    // Extend the graph with new package
    {
        let mut graph = state.graph().write().unwrap();
        let start_time = PreciseTime::now();
        let (ncount, ecount) = graph.extend(&package);
        let end_time = PreciseTime::now();

        println!("Extended graph, nodes: {}, edges: {} ({} sec)\n",
                 ncount,
                 ecount,
                 start_time.to(end_time));
    };

    try!(req.reply_complete(sock, &package));
    Ok(())
}

pub fn package_stats_get(req: &mut Envelope,
                         sock: &mut zmq::Socket,
                         state: &mut ServerState)
                         -> Result<()> {
    let msg: proto::PackageStatsGet = try!(req.parse_msg());
    println!("package_stats_get message: {:?}", msg);

    match state.datastore().get_package_stats(&msg) {
        Ok(package_stats) => try!(req.reply_complete(sock, &package_stats)),
        Err(err) => {
            error!("Unable to retrieve package stats for {}, err: {:?}",
                   msg.get_origin(),
                   err);
            let err = net::err(ErrCode::ENTITY_NOT_FOUND, "sc:package-stats-get:1");
            try!(req.reply_complete(sock, &err));
        }
    };

    Ok(())
}
