#!/usr/bin/env python3
"""Validate the shipped native level without the game or graphics libraries."""
import heapq
import itertools
import json
import math
from pathlib import Path

LEVEL = Path(__file__).resolve().parent.parent/'assets/level.json'
TURN_RADIUS = 3.7  # clearance for a rotating 6.35 × 3.15 m tracked vehicle

def local(p,x,z):
    c,s=math.cos(p.get('yaw',0)),math.sin(p.get('yaw',0))
    dx,dz=x-p['x'],z-p['z']
    return (c*dx+s*dz,-s*dx+c*dz)

def contains(p,x,z,r=0):
    a,b=local(p,x,z)
    return math.hypot(max(0,abs(a)-p.get('collisionW',p['w'])/2),
                      max(0,abs(b)-p.get('collisionD',p['d'])/2)) <= r

def blocks_sight(p,a,b):
    if p['h'] < 1.68: return False
    entry,exit=0.,1.
    local_a,local_b=local(p,*a),local(p,*b)
    for x0,x1,width in zip(local_a,local_b,(p.get('collisionW',p['w']),p.get('collisionD',p['d']))):
        d=x1-x0
        if abs(d)<1e-10:
            if abs(x0)>width/2:return False
        else:
            t0,t1=sorted(((-width/2-x0)/d,(width/2-x0)/d))
            entry=max(entry,t0);exit=min(exit,t1)
            if entry>exit:return False
    return True

def pos(p):return (p['x'],p['z'])
def clear_corridor(a,b,radius,solids):
    length=math.dist(a,b);steps=max(1,math.ceil(length/.7))
    for p in solids:
        ext=radius+math.hypot(p.get('collisionW',p['w']),p.get('collisionD',p['d']))*.5
        if (p['x']<min(a[0],b[0])-ext or p['x']>max(a[0],b[0])+ext or
            p['z']<min(a[1],b[1])-ext or p['z']>max(a[1],b[1])+ext):continue
        for i in range(steps+1):
            t=i/steps
            if contains(p,a[0]+(b[0]-a[0])*t,a[1]+(b[1]-a[1])*t,radius):return p
    return None
def distances(graph,start,ignore=None):
    found={start:0.}; heap=[(0.,start)]
    while heap:
        distance,src=heapq.heappop(heap)
        if distance>found[src]:continue
        for dst,cost in graph[src]:
            if {src,dst}==ignore:continue
            total=distance+cost
            if total<found.get(dst,math.inf):
                found[dst]=total;heapq.heappush(heap,(total,dst))
    return found
def check(m):
    assert len(m['props'])<=450
    solids=[p for p in m['props'] if p.get('solid')]
    starts=m['teamSpawns'];motor=m['vehicleSpawns']
    assert math.dist(pos(starts['blue'][0]),pos(starts['red'][0]))>=400
    for color,spots in starts.items():
        assert len(spots)>=6 and all(not any(contains(p,*pos(s),.52) for p in solids) for s in spots)
        for k in ['wood','metal','ammo','gunpowder']:
            assert any(p.get('resource')==k and p.get('stock',0)>0 and min(math.dist(pos(p),pos(s)) for s in spots)<12
                       for p in solids),(color,k)
        vehicles=[p for p in motor if p['team']==color]
        assert len(vehicles)==5 and {p['kind'] for p in vehicles}=={'patrol4x4','cargo6x6','mbt','scout_buggy','apc'}
        for v in vehicles:
            obstacle=next((p for p in solids if contains(p,*pos(v),TURN_RADIUS)),None)
            assert not obstacle,(v,obstacle)
            assert all(math.dist(pos(v),pos(other))>2*TURN_RADIUS for other in vehicles if v is not other)
            entry=pos(v['exit'])
            obstacle=clear_corridor(pos(v),entry,TURN_RADIUS,solids)
            assert not obstacle,('vehicle exit',v['kind'],color,obstacle)
    for a,b in itertools.product(starts['blue'],starts['red']):
        assert any(blocks_sight(p,pos(a),pos(b)) for p in solids),(a,b,'uncovered spawn pair')
    for y in m['workyards']:
        assert not any(contains(y,*pos(p),math.hypot(p['collisionW'],p['collisionD'])*.5)
                       for p in solids),('obstructed construction pad',y['team'])
    graphdata=m['routeGraph'];nodes={k:pos(v) for k,v in graphdata['nodes'].items()}
    adj={k:[] for k in nodes};bad=[]
    for a,b in graphdata['edges']:
        # Check the painted road points along the same drive corridor as the graph.
        mid=tuple((x+y)*.5 for x,y in zip(nodes[a],nodes[b]))
        assert any(math.dist(pos(r),mid)<.02 and r['w']>=2*TURN_RADIUS and
                   max(abs(local(r,*nodes[a])[0]),abs(local(r,*nodes[b])[0]))<.01 and
                   math.dist(nodes[a],nodes[b])<=r['d'] for r in m['roads']),('missing road surface',a,b)
        obstacle=clear_corridor(nodes[a],nodes[b],TURN_RADIUS,solids)
        if obstacle:bad.append((a,b,obstacle['kind'],pos(obstacle)))
        length=math.dist(nodes[a],nodes[b]);adj[a].append((b,length));adj[b].append((a,length))
    assert not bad,('roads obstructed',bad)
    points=m['resourcePoints']
    assert len({p['id'] for p in points})==len(points)
    for point in points:
        assert point['name'] and 0<point['initial']<=point['capacity']<=32 and point['interval']>=8
        assert not any(contains(p,*pos(point),.55) for p in solids),('occupied resource site',point)
        # The resource has a walkable direct path to a real driving route.
        def nearest(a,b):
            vx,vz=b[0]-a[0],b[1]-a[1]
            t=max(0,min(1,((point['x']-a[0])*vx+(point['z']-a[1])*vz)/(vx*vx+vz*vz)))
            return (a[0]+t*vx,a[1]+t*vz)
        candidates=sorted((nearest(nodes[a],nodes[b]) for a,b in graphdata['edges']),key=lambda p:math.dist(p,pos(point)))
        assert any(math.dist(c,pos(point))<=32 and not clear_corridor(c,pos(point),.4,solids)
                   for c in candidates),('no access from road',point)
    for color in ('blue','red'):
        sites=[p for p in points if p['id'].startswith(color+'-')]
        assert {p['resource'] for p in sites}=={'wood','metal','oil','fuel','powder'}
        assert sum(p['resource']=='metal' for p in sites)>=3
    oil=[p for p in m['props'] if p.get('resource')=='oil']
    assert len(oil)==2 and all(contains(p,*pos(p)) and p.get('stock')>=120 for p in oil)
    for pump in oil:
        junction=min((nodes[k] for k in ('blue/oil','red/oil')),key=lambda pt:math.dist(pt,pos(pump)))
        assert math.dist(junction,pos(pump))<18
        # Dismount on the verge and walk to a point within the interaction
        # reach, staying outside the rig and away from the equipment shed.
        walker_clear=False
        for angle in range(0,360,15):
            a=math.radians(angle);r=max(pump['w'],pump['d'])/2+1.1
            stand=(pump['x']+r*math.cos(a),pump['z']+r*math.sin(a))
            if not clear_corridor(junction,stand,.52,solids):
                walker_clear=True;break
        assert walker_clear,('unreachable pump',pos(pump))
    # Every fuel stop must remain reachable if any individual road is closed.
    for edge in graphdata['edges']:
        for color in ('blue','red'):
            reachable=distances(adj,color+'/hq',ignore=set(edge))
            assert all(k in reachable for k in ('blue/oil','red/oil',{'blue':'red','red':'blue'}[color]+'/hq'))
    for color in ('blue','red'):
        d=distances(adj,color+'/hq'); print(color,'to pumps:',round(d['blue/oil']),round(d['red/oil']),
            'metres; opposing base:',round(d[{'blue':'red','red':'blue'}[color]+'/hq']))
    print(f"{len(solids)} solid props; {len(graphdata['edges'])} tested roads; "
          f"{len(starts['blue'])*len(starts['red'])} spawn sightline checks")
if __name__=='__main__':check(json.loads(LEVEL.read_text()))
