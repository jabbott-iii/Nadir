# nadir-ecs

An entity-component system: generational entities, components in sparse
sets, resources, events, deferred commands and a schedule of systems in
stages. It is small, single-threaded and has no `unsafe` code.

It is part of [Nadir](https://github.com/jabbott-iii/Nadir), a 2D engine for
top-down games on Windows, macOS and Linux, written in Rust with no
third-party crates. Games use it through
[nadir_engine](https://crates.io/crates/nadir_engine), which re-exports it as
`nadir::ecs`; depend on it directly to use it on its own.

## Status

Version 0.3: the API will change before 1.0.

Built today:

- entities as a slot and a generation, so that an old `Entity` never reads
  the components of a newer one in its slot;
- one sparse set per component type, borrowed at run time, with helpers to
  iterate two types together;
- resources, events by tick and by frame, and commands for spawning,
  despawning, adding and removing components;
- a schedule that runs systems once a fixed tick and once a frame, in
  stages, with optional run conditions.

By design there is no parallelism and no automatic ordering of systems:
they run in stage order, then in the order they were added.

## Usage

```toml
[dependencies]
nadir-ecs = "0.3"
# Systems take nadir-core's `Tick` or `Frame`.
nadir-core = "0.3"
```

Components are plain types that implement `Component`. A system is a
function of the world and the tick:

```rust
use nadir_core::time::Tick;
use nadir_ecs::{Component, FixedStage, Schedule, World, for_each_mut_with};

struct Position(f32);
struct Velocity(f32);
impl Component for Position {}
impl Component for Velocity {}

fn movement(world: &World, tick: &Tick) {
    let velocity = world.read::<Velocity>();
    let mut position = world.write::<Position>();
    for_each_mut_with(&mut position, &velocity, |_, p, v| p.0 += v.0 * tick.dt);
}

let mut world = World::new();
world.register::<Position>();
world.register::<Velocity>();
let ball = world.spawn().with(Position(0.0)).with(Velocity(30.0)).id();
let wall = world.spawn().with(Position(100.0)).id();

let mut schedule = Schedule::new();
schedule.add_fixed(FixedStage::Movement, "movement", movement);
for index in 0..60 {
    schedule.run_fixed(&mut world, &Tick { index, dt: 1.0 / 60.0 });
}

// One second at 30 pixels a second. The wall has no velocity.
let position = world.read::<Position>();
assert!((position.get(ball).map_or(0.0, |p| p.0) - 30.0).abs() < 1e-3);
assert_eq!(position.get(wall).map(|p| p.0), Some(100.0));
```

Systems change the world's structure through commands, which wait until
the schedule applies them after the tick. Events go to the systems that run
after the sender:

```rust
use nadir_ecs::{Component, Entity, Events, World};

struct Health(i32);
impl Component for Health {}

#[derive(Clone, Debug, PartialEq)]
struct Died(Entity);

fn remove_the_dead(world: &World) {
    let mut commands = world.commands();
    let mut died = world.resource_mut::<Events<Died>>();
    for (entity, health) in world.read::<Health>().iter() {
        if health.0 <= 0 {
            died.send(Died(entity));
            commands.despawn(entity);
        }
    }
}

let mut world = World::new();
world.register::<Health>();
world.add_event::<Died>();
let slime = world.spawn().with(Health(0)).id();

remove_the_dead(&world);
assert!(world.is_alive(slime), "commands wait to be applied");
world.apply_commands();
assert!(!world.is_alive(slime));
assert_eq!(world.resource::<Events<Died>>().tick_events(), &[Died(slime)]);
```

Borrowing a component type or a resource that is already borrowed mutably
panics, with a message that names the type and the running system. So does
using a component type that is not registered.

## Documentation

- API reference: https://docs.rs/nadir-ecs
- Design: https://github.com/jabbott-iii/Nadir/blob/main/docs/subsystems/ecs.md
- The engine: https://github.com/jabbott-iii/Nadir

## License

Apache License 2.0: see [LICENSE](LICENSE) and [NOTICE](NOTICE).
