use std::collections::{BTreeSet, HashMap};

use brb::{Actor, BRBDataType};

use super::{Money, Op, Transfer};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bank {
    id: Actor,
    // The set of dependencies of the next outgoing transfer
    deps: BTreeSet<Transfer>,

    // The initial balances when opening an actor opened an account
    initial_balances: HashMap<Actor, Money>,

    // Set of all transfers impacting a given actor
    hist: HashMap<Actor, BTreeSet<Transfer>>,
}

impl Bank {
    pub fn open_account(&self, owner: Actor, balance: Money) -> Op {
        Op::OpenAccount { owner, balance }
    }

    pub fn initial_balance(&self, actor: &Actor) -> Money {
        self.initial_balances
            .get(&actor)
            .cloned()
            .unwrap_or_else(|| panic!("[ERROR] No initial balance for {}", actor))
    }

    pub fn balance(&self, actor: &Actor) -> Money {
        // TODO: in the paper, when we read from an actor, we union the actor
        //       history with the deps, I don't see a use for this since anything
        //       in deps is already in the actor history. Think this through a
        //       bit more carefully.
        let h = self.history(actor);

        let outgoing: Money = h
            .iter()
            .filter(|t| &t.from == actor)
            .map(|t| t.amount)
            .sum();
        let incoming: Money = h.iter().filter(|t| &t.to == actor).map(|t| t.amount).sum();

        // We compute differences in a larger space since we need to move to signed numbers
        // and hence we lose a bit.
        let balance_delta: i128 = (incoming as i128) - (outgoing as i128);
        let balance: i128 = self.initial_balance(actor) as i128 + balance_delta;

        assert!(balance >= 0); // sanity check that we haven't violated our balance constraint
        assert!(balance <= Money::max_value() as i128); // sanity check that it's safe to downcast

        balance as Money
    }

    fn history(&self, actor: &Actor) -> BTreeSet<Transfer> {
        self.hist.get(&actor).cloned().unwrap_or_default()
    }

    pub fn transfer(&self, from: Actor, to: Actor, amount: Money) -> Option<Op> {
        let balance = self.balance(&from);
        // TODO: we should leave this validation to the self.validate logic, no need to duplicate it here
        if balance < amount {
            println!(
                "{} does not have enough money to transfer {} to {}. (balance: {})",
                from, amount, to, balance
            );
            None
        } else {
            let deps = self.deps.clone();
            Some(Op::Transfer(Transfer {
                from,
                to,
                amount,
                deps,
            }))
        }
    }
}

impl BRBDataType for Bank {
    type Op = Op;

    fn new(id: Actor) -> Self {
        Bank {
            id,
            deps: Default::default(),
            initial_balances: Default::default(),
            hist: Default::default(),
        }
    }

    /// Protection against Byzantines
    fn validate(&self, from: &Actor, op: &Op) -> bool {
        let validation_tests = match op {
            Op::Transfer(transfer) => vec![
                (
                    from == &transfer.from,
                    "Sender initiated transfer on behalf of other proc",
                ),
                (
                    self.initial_balances.contains_key(&transfer.from),
                    "From account does not exist",
                ),
                (
                    self.initial_balances.contains_key(&transfer.to),
                    "To account does not exist",
                ),
                (
                    self.balance(from) >= transfer.amount,
                    "Sender has insufficient funds",
                ),
                (
                    transfer.deps.is_subset(&self.history(from)),
                    "Missing dependent ops",
                ),
            ],
            Op::OpenAccount { owner, balance: _ } => vec![
                (
                    from == owner,
                    "Initiator is not the owner of the new account",
                ),
                (
                    !self.initial_balances.contains_key(owner),
                    "Owner already has an account",
                ),
            ],
        };

        validation_tests
            .into_iter()
            .find(|(is_valid, _msg)| !is_valid)
            .map(|(_test, msg)| println!("[BANK/VALIDATION] {} {:?}, {:?}", msg, op, self))
            .is_none()
    }

    /// Executed once an op has been validated
    fn apply(&mut self, op: Op) {
        match op {
            Op::Transfer(transfer) => {
                // Update the history for the outgoing account
                self.hist
                    .entry(transfer.from)
                    .or_default()
                    .insert(transfer.clone());

                // Update the history for the incoming account
                self.hist
                    .entry(transfer.to)
                    .or_default()
                    .insert(transfer.clone());

                if transfer.to == self.id {
                    self.deps.insert(transfer.clone());
                }

                if transfer.from == self.id {
                    // In the paper, deps are cleared after the broadcast completes in
                    // self.transfer.
                    // Here we break up the initiation of the transfer from the completion.
                    // We move the clearing of the deps here since this is where we now know
                    // the transfer was successfully validated and applied by the network.
                    for prior_transfer in transfer.deps.iter() {
                        // for each dependency listed in the transfer
                        // we remove it from the set of dependencies for a transfer
                        self.deps.remove(prior_transfer);
                    }
                }
            }
            Op::OpenAccount { owner, balance } => {
                println!("[BANK] opening new account for {} with ${}", owner, balance);
                self.initial_balances.insert(owner, balance);
            }
        }
    }
}
