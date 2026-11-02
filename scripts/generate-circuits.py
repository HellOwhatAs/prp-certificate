import pathlib,json
D=json.loads(pathlib.Path('public/data/instance.json').read_text())
F=256
body=r'''
fn matches(e: u32, p: u32) -> bool { ((e == 55) & (p < 3)) | (e == p) }
fn main(
    // Scores and the human-readable certificate identity are public inputs.
    identity: pub [u8; 32], mode: pub u32, score: pub [u32; 9],
    resources: [u32; F], lengths: [u32; F],
    loc: [u32; S], refuel: [bool; S], times_in: [u32; S], times_out: [u32; S],
    served: [bool; P], flight: [u32; P], pickup: [u32; P], delivery: [u32; P],
) {
    // Constrain identity bytes to a public/private relation so they cannot be dropped.
    let identity_copy = identity;
    assert(identity_copy == identity);
    assert(mode <= 1);
    if Q != 3 { assert(mode == 1); }
    let mut arrivals: [u32; S] = [0; S];
    let mut departures: [u32; S] = [0; S];
    let mut last_end: [u32; 24] = [0; 24];
    let mut has_flight: [bool; 24] = [false; 24];
    let mut total = 0;
    let mut fuel_total = 0;
    let mut flights = 0;
    let mut landings = 0;
    for f in 0..F {
        let n = lengths[f];
        let active = n != 0;
        assert((n == 0) | ((n >= 3) & (n <= 7)));
        assert(resources[f] < (if Q == 3 {24} else {3}));
        let t = if Q == 3 { PLANE_TYPE[resources[f]] } else { resources[f] };
        let base = loc[f*7];
        let mut remaining = TANK[t];
        let mut end_time = 0;
        for k in 0..7 {
            let idx = f*7+k;
            assert(loc[idx] < 55);
            if k < n {
                if k == 0 {
                    assert(base < 3);
                    if Q == 3 { assert(base == PLANE_HOME[resources[f]]); }
                    assert(!refuel[idx]);
                    if Q == 3 {
                        assert(times_in[idx] == 0);
                        assert(times_out[idx] <= 10080);
                        let clock = times_out[idx] % 1440;
                        assert(clock >= 360 & clock <= 1080);
                        departures[idx] = times_out[idx];
                    }
                } else {
                    let distance = DIST[loc[idx-1]*55+loc[idx]];
                    assert(loc[idx] != loc[idx-1]);
                    let fuel = distance * RATE[t];
                    assert(remaining >= fuel + RESERVE[t]);
                    remaining = remaining - fuel;
                    fuel_total = fuel_total + fuel;
                    let fly = (60 * distance + SPEED[t] - 1) / SPEED[t];
                    arrivals[idx] = departures[idx-1] + fly;
                    if Q == 3 {
                        assert(times_in[idx] == arrivals[idx]);
                        assert(times_in[idx] <= 10080);
                    }
                    if k + 1 == n {
                        assert(loc[idx] == base);
                        assert(!refuel[idx]);
                        if Q == 3 {
                            assert(times_out[idx] == 0);
                            assert(arrivals[idx] / 1440 == departures[f*7] / 1440);
                            assert(arrivals[idx] % 1440 <= 1200);
                        }
                        end_time = arrivals[idx];
                    } else {
                        assert(loc[idx] >= 3);
                        if refuel[idx] {
                            assert(IS_REFUEL[loc[idx]]);
                            remaining = TANK[t];
                        }
                        let dwell = if refuel[idx] {20} else {10};
                        if Q == 3 {
                            assert(times_out[idx] >= arrivals[idx] + dwell);
                            assert(times_out[idx] <= 10080);
                            departures[idx] = times_out[idx];
                        } else { departures[idx] = arrivals[idx] + dwell; }
                    }
                }
            } else {
                assert((loc[idx] == 0) & !refuel[idx]);
                assert((times_in[idx] == 0) & (times_out[idx] == 0));
            }
            if Q != 3 { assert((times_in[idx] == 0) & (times_out[idx] == 0)); }
        }
        if active {
            if Q == 3 {
                let r = resources[f];
                if has_flight[r] { assert(departures[f*7] >= last_end[r]+30); }
                has_flight[r] = true;
                last_end[r] = end_time;
            }
            total = total + end_time - departures[f*7];
            flights = flights + 1;
            landings = landings + n - 2;
        }
    }
    let mut board: [u32; S] = [0; S];
    let mut alight: [u32; S] = [0; S];
    let mut person_time = 0;
    let mut temporary = 0;
    for p in 0..P {
        let f = flight[p];
        assert(f < F);
        assert(pickup[p] < 7 & delivery[p] < 7);
        if served[p] {
            assert(lengths[f] > 0);
            assert(pickup[p] < delivery[p] & delivery[p] < lengths[f]);
            let a = f*7+pickup[p];
            let b = f*7+delivery[p];
            assert(matches(ORIGIN[p],loc[a]));
            assert(matches(DESTINATION[p],loc[b]));
            for k in 0..7 {
                if k > pickup[p] & k < delivery[p] { assert(!matches(DESTINATION[p], loc[f*7+k])); }
            }
            if Q == 3 {
                assert(departures[a] >= EARLIEST[p]);
                assert(arrivals[b] <= LATEST[p]);
                if TEMP[p] { assert(mode == 1); temporary = temporary + 1; }
            }
            person_time = person_time + arrivals[b] - departures[a];
            board[a] = board[a] + 1;
            alight[b] = alight[b] + 1;
        } else {
            assert((Q == 3) & TEMP[p]);
            assert((f == 0) & (pickup[p] == 0) & (delivery[p] == 0));
        }
    }
    let mut passenger_km = 0;
    let mut seat_km = 0;
    let mut empty_km = 0;
    for f in 0..F {
        let n = lengths[f];
        let t = if Q == 3 { PLANE_TYPE[resources[f]] } else { resources[f] };
        let mut load = 0;
        for k in 0..7 {
            let idx = f*7+k;
            assert(load >= alight[idx]);
            load = load - alight[idx] + board[idx];
            assert(load <= CAPACITY[t]);
            if k + 1 < n {
                let d = DIST[loc[idx]*55+loc[idx+1]];
                passenger_km = passenger_km + load*d;
                seat_km = seat_km + CAPACITY[t]*d;
                if load == 0 { empty_km = empty_km + d; }
            }
        }
        assert(load == 0);
    }
    assert(flights > 0);
    assert(score == [total,person_time,fuel_total,flights,landings,passenger_km,seat_km,temporary,empty_km]);
}
'''
def array(name,arr,typ='u32'):
 vals=','.join(str(x).lower() for x in arr)
 return f'global {name}: [{typ}; {len(arr)}] = [{vals}];\n'
for q in (1,2,3):
 p=D['people'][str(q)]
 const=f'global Q: u32 = {q};\nglobal F: u32 = {F};\nglobal S: u32 = {F*7};\nglobal P: u32 = {len(p)};\n'
 const+=array('DIST',sum(D['distance'],[]))
 for name,key in [('PLANE_TYPE','planeType'),('PLANE_HOME','planeHome'),('CAPACITY','capacity'),('SPEED','speed'),('RATE','fuelRate10'),('TANK','tank10'),('RESERVE','reserve10')]: const+=array(name,D[key])
 const+=array('IS_REFUEL',[i in D['refuel'] for i in range(55)],'bool')
 for name,key in [('ORIGIN','origin'),('DESTINATION','destination'),('EARLIEST','earliest'),('LATEST','latest')]: const+=array(name,[x[key] for x in p])
 const+=array('TEMP',[x['task']=='temporary' for x in p],'bool')
 folder=pathlib.Path(f'circuits/q{q}'); (folder/'src').mkdir(parents=True,exist_ok=True)
 (folder/'src/main.nr').write_text(const+body)
 (folder/'Nargo.toml').write_text(f'[package]\nname="q{q}"\ntype="bin"\nauthors=[""]\ncompiler_version=">=1.0.0"\n')
