#[cfg(test)]
#[cfg(feature = "expensive_tests")]
mod tests {
    use near_chain::{Doomslug, DoomslugThresholdMode};
    use near_crypto::{InMemorySigner, KeyType, SecretKey};
    use near_primitives::block::Approval;
    use near_primitives::hash::{hash, CryptoHash};
    use near_primitives::types::{BlockHeight, ValidatorStake};
    use rand::{thread_rng, Rng};
    use std::collections::{HashMap, HashSet};
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    fn block_hash(height: BlockHeight) -> CryptoHash {
        hash(height.to_le_bytes().as_ref())
    }

    fn get_msg_delivery_time(now: Instant, gst: Instant, delta: Duration) -> Instant {
        std::cmp::max(now, gst)
            + Duration::from_millis(thread_rng().gen_range(0, delta.as_millis()) as u64)
    }

    /// Runs a single iteration of a fuzz test given specific time until global stabilization and
    /// the max delay on messages.
    /// Returns amount of time it took to produce a doomslug final block at height 50, as well as the
    /// largest height encountered
    ///
    /// # Arguments
    /// * `time_to_gst` - number of milliseconds before global stabilization time
    /// * `delta`       - max message delay
    /// * `height_goal` - the appearance of a block at this (or heigher) height with doomslug finality
    ///                   will end the test
    fn one_iter(
        time_to_gst: Duration,
        delta: Duration,
        height_goal: BlockHeight,
    ) -> (Duration, BlockHeight) {
        let account_ids =
            vec!["test1", "test2", "test3", "test4", "test5", "test6", "test7", "test8"];
        let stakes = account_ids
            .iter()
            .map(|account_id| ValidatorStake {
                account_id: account_id.to_string(),
                stake: 1,
                public_key: SecretKey::from_seed(KeyType::ED25519, account_id).public_key(),
            })
            .collect::<Vec<_>>();
        let signers = account_ids
            .iter()
            .map(|account_id| {
                Arc::new(InMemorySigner::from_seed(account_id, KeyType::ED25519, account_id))
            })
            .collect::<Vec<_>>();
        let mut doomslugs = account_ids
            .iter()
            .zip(signers.iter())
            .map(|(account_id, signer)| {
                Doomslug::new(
                    Some(account_id.to_string()),
                    0,
                    0,
                    Duration::from_millis(200),
                    Duration::from_millis(1000),
                    Duration::from_millis(100),
                    delta * 20, // some arbitrary number larger than delta * 6
                    Some(signer.clone()),
                    DoomslugThresholdMode::HalfStake,
                )
            })
            .collect::<Vec<_>>();

        let mut now = Instant::now();
        let started = now;

        let gst = now + time_to_gst;
        let mut approval_queue: Vec<(Approval, Instant)> = vec![];
        let mut block_queue: Vec<(BlockHeight, usize, BlockHeight, Instant)> = vec![];
        let mut largest_produced_height: BlockHeight = 1;
        let mut chain_lengths = HashMap::new();
        let mut hash_to_block_info: HashMap<CryptoHash, (BlockHeight, BlockHeight)> =
            HashMap::new();
        let mut hash_to_prev_hash: HashMap<CryptoHash, CryptoHash> = HashMap::new();

        let mut blocks_with_doomslug_finality: Vec<BlockHeight> = vec![];

        chain_lengths.insert(block_hash(1), 1);

        for ds in doomslugs.iter_mut() {
            ds.set_tip(now, block_hash(1), None, 1, 1);
            hash_to_block_info.insert(block_hash(1), (1, 1));
        }

        let mut is_done = false;
        while !is_done {
            now = now + Duration::from_millis(25);
            let mut new_approval_queue = vec![];
            let mut new_block_queue = vec![];

            // 1. Process approvals
            for mut approval in approval_queue.into_iter() {
                if approval.1 > now {
                    new_approval_queue.push(approval);
                } else {
                    let me = (approval.0.target_height % 8) as usize;

                    // Make test1 be malicious and always send skips
                    if approval.0.account_id == "test1" {
                        approval.0.is_endorsement = false;
                    }

                    // Make test2 be offline and never send approvals
                    if approval.0.account_id == "test2" {
                        continue;
                    }

                    // Generally make 20% of the remaining approvals to drop
                    if thread_rng().gen_range(0, 10) < 2 {
                        continue;
                    }

                    doomslugs[me].on_approval_message(now, &approval.0, &stakes);
                }
            }
            approval_queue = new_approval_queue;

            // 2. Process blocks
            for block in block_queue.into_iter() {
                if block.3 > now {
                    new_block_queue.push(block);
                } else {
                    let ds = &mut doomslugs[block.1 as usize];
                    if block.0 as BlockHeight > ds.get_tip().1 {
                        // Accept all the blocks from the tip till this block
                        let mut block_infos = vec![(block.0, block.2)];
                        for block_index in 0..50 {
                            if block_index == 49 {
                                assert!(false);
                            }

                            let last_block = block_infos.last().unwrap();
                            let prev_block_info = hash_to_block_info
                                .get(&hash_to_prev_hash.get(&block_hash(last_block.0)).unwrap())
                                .unwrap();

                            if prev_block_info.0 as BlockHeight <= ds.get_tip().1 {
                                break;
                            }
                            block_infos.push(*prev_block_info);
                        }

                        for block_info in block_infos.into_iter().rev() {
                            ds.set_tip(
                                now,
                                block_hash(block_info.0),
                                None,
                                block_info.0 as BlockHeight,
                                block_info.1,
                            );
                        }
                    }
                }
            }
            block_queue = new_block_queue;

            // 3. Process timers
            for ds in doomslugs.iter_mut() {
                for approval in ds.process_timer(now) {
                    approval_queue.push((approval, get_msg_delivery_time(now, gst, delta)));
                }
            }

            // 4. Produce blocks
            'outer: for ds in doomslugs.iter_mut() {
                for target_height in
                    (ds.get_tip().1 + 1)..=ds.get_largest_height_crossing_threshold()
                {
                    if ds.ready_to_produce_block(now, target_height, true) {
                        let parent_hash = ds.get_tip().0;

                        let is_ds_final = ds.is_prev_block_ds_final(parent_hash, target_height);

                        let last_ds_final_height = if is_ds_final {
                            target_height
                        } else {
                            hash_to_block_info.get(&parent_hash).unwrap().1
                        };

                        if target_height >= 512 {
                            println!("Largest produced_height: {}", largest_produced_height);
                            for ds in doomslugs.iter() {
                                println!(
                                    "  - tip: ({:?}), ds_final_height: {}, timer height: {}",
                                    ds.get_tip(),
                                    ds.get_largest_height_with_doomslug_finality(),
                                    ds.get_timer_height()
                                );
                            }
                            assert!(false);
                            break 'outer;
                        }
                        let block_hash = block_hash(target_height);
                        for whom in 0..8 {
                            let block_info = (
                                target_height,
                                whom,
                                last_ds_final_height,
                                get_msg_delivery_time(now, gst, delta),
                            );
                            block_queue.push(block_info);
                        }

                        hash_to_block_info
                            .insert(block_hash, (target_height, last_ds_final_height));
                        hash_to_prev_hash.insert(block_hash, parent_hash);

                        assert!(chain_lengths.get(&block_hash).is_none());
                        let prev_length = *chain_lengths.get(&ds.get_tip().0).unwrap();
                        chain_lengths.insert(block_hash, prev_length + 1);

                        if is_ds_final {
                            blocks_with_doomslug_finality.push(target_height - 1);
                        }

                        if target_height > largest_produced_height {
                            largest_produced_height = target_height;
                        }
                        if target_height >= height_goal && is_ds_final {
                            assert!(prev_length + 1 > 20); // make sure we actually built some chain
                            is_done = true;
                        }

                        // Accept our own block
                        ds.set_tip(
                            now,
                            block_hash,
                            None,
                            target_height as BlockHeight,
                            last_ds_final_height,
                        );
                    }
                }
            }

            // 5. In the liveness proof we rely on timers always being within delta from each other
            //    Validate that assumption
            for i in 0..8 {
                for j in (i + 1)..8 {
                    let ith_timer_start = doomslugs[i].get_timer_start();
                    let jth_timer_start = doomslugs[j].get_timer_start();

                    // Only makes sense for timers that are more than delta in the past, since for more
                    // recent timers the other participant's start time might be in the future
                    if now - ith_timer_start >= delta && now - jth_timer_start >= delta {
                        if ith_timer_start > jth_timer_start {
                            assert!(ith_timer_start - jth_timer_start <= delta);
                        } else {
                            assert!(jth_timer_start - ith_timer_start <= delta);
                        }
                    }
                }
            }
        }

        // We successfully got to the `height_goal`. Check that all the blocks are building only on
        // doomslug final blocks
        for (block_hash, (block_height, _)) in hash_to_block_info.iter() {
            let mut seen_heights = HashSet::new();
            let mut block_hash = block_hash.clone();
            seen_heights.insert(*block_height);

            loop {
                match hash_to_prev_hash.get(&block_hash) {
                    None => break,
                    Some(prev_block_hash) => {
                        block_hash = *prev_block_hash;
                        seen_heights.insert(hash_to_block_info.get(&block_hash).unwrap().0);
                    }
                }
            }

            for height in blocks_with_doomslug_finality.iter() {
                assert!(*height > *block_height || seen_heights.contains(height));
            }
        }

        (now - started, largest_produced_height)
    }

    #[test]
    fn test_fuzzy_doomslug_liveness_and_safety() {
        for (time_to_gst_millis, height_goal) in
            &[(0, 100), (1000, 100), (10000, 100), (100000, 200), (500000, 300)]
        {
            for delta in &[100, 300, 500, 1000, 2000] {
                println!(
                    "Staring set of tests. Time to GST: {}, delta: {}",
                    time_to_gst_millis, delta
                );
                for _iter in 0..10 {
                    let (took, height) = one_iter(
                        Duration::from_millis(*time_to_gst_millis),
                        Duration::from_millis(*delta),
                        *height_goal,
                    );
                    println!(
                        " --> Took {} (simulated) milliseconds and {} heights",
                        took.as_millis(),
                        height
                    );
                }
            }
        }
    }
}
