#!/usr/bin/env python3
"""Generate a contemporary industrial valley around the preserved village of Saint-Laurent.
Run from anywhere with Python 3. All lengths are metres; +Z is south.
"""
import json
import math
import random
from pathlib import Path

HERE = Path(__file__).resolve().parent
CORE = HERE / 'art-source/village-core.json'
OUT = HERE.parent / 'assets/level.json'

NODES = {
    'hq':(-208,130), 'garrison':(-205,82), 'riverbend':(-212,21),
    'quarry':(-171,-48), 'cutting':(-128,-102), 'oil':(-83,-146),
    'mill':(-148,142), 'vines':(-106,105), 'garden':(-54,62),
    'cross':(-66,1), 'hamlet':(-118,8), 'fork':(-123,68),
    'wheat':(-45,-100), 'gate':(-48,-56), 'square':(-3,-52),
    'upland':(-10,-171), 'ridge':(66,-173), 'yard':(-193,171),
    'village':(-41,-8), 'park':(-240,128), 'westpark':(-249,109), 'perimeter':(-240,80),
    'annex':(-249,189), 'backroad':(-214,193), 'spur':(-190,189),
    'westgate':(-264,157), 'westturn':(-264,127),
}
# The names identify junctions in the blue half. A leading "+" references
# the corresponding junction in the red half. Both halves share ring roads.
SEGMENTS = [
    ('hq','garrison',12),('garrison','riverbend',12),('riverbend','quarry',12),
    ('quarry','cutting',11),('cutting','oil',11),('hq','mill',13),
    ('mill','vines',11),('vines','garden',11),('garden','cross',12),
    ('vines','fork',9),('fork','garrison',10),('fork','hamlet',10),
    ('hamlet','cross',11),('hamlet','quarry',10),('oil','wheat',10),
    ('wheat','gate',11),('gate','cross',12),('gate','square',10),
    ('square','+garden',11),('cross','village',8),('oil','upland',12),
    ('upland','ridge',12),('ridge','+mill',12),('hq','yard',10),
    ('yard','mill',11),('park','hq',11),('park','westpark',11),
    ('westpark','perimeter',10),('perimeter','garrison',11),
    ('annex','backroad',10),('backroad','spur',10),('spur','yard',10),
    ('annex','westgate',10),('westgate','westturn',10),('westturn','westpark',10),
]

def center_of_segment(a,b):
    # Local +Z transforms to (-sin(yaw),cos(yaw)).
    x1,z1=a; x2,z2=b
    return ((x1+x2)*.5,(z1+z2)*.5,math.hypot(x2-x1,z2-z1),math.atan2(x1-x2,z2-z1))

def point_on_segment_distance(x,z,a,b):
    dx, dz = b[0]-a[0], b[1]-a[1]
    t=max(0,min(1,((x-a[0])*dx+(z-a[1])*dz)/(dx*dx+dz*dz)))
    return math.hypot(x-a[0]-dx*t,z-a[1]-dz*t)

def generate():
    level=json.loads(CORE.read_text())
    props,roads,fields=level['props'],level['roads'],level['fields']
    level.update(name='Сен-Лоран · Промышленная долина',era='21st century',bounds=dict(minX=-280,maxX=280,minZ=-230,maxZ=230),
                 teamSpawns={'blue':[],'red':[]},vehicleSpawns=[],landmarks=[],workyards=[],resourcePoints=[])
    level.pop('enemySpawns',None)
    level['objective'].update(kind='telecom',name='Ретранслятор долины')
    routes=[]
    network={}
    edges=[]
    yards=[]
    for team,side in [('blue',1),('red',-1)]:
        for name,(x,z) in NODES.items():
            network[f'{team}/{name}']=(side*x,side*z)
    def road(a,b,w,name=None):
        x,z,d,yaw=center_of_segment(a,b)
        roads.append(dict(x=x,z=z,w=w,d=round(d+1.5,2),yaw=round(yaw,6),**({'name':name} if name else {})))
        routes.append((a,b,w))
    for team,other in [('blue','red'),('red','blue')]:
        for start,end,w in SEGMENTS:
            a=f'{team}/{start}'
            b=f'{other}/{end[1:]}' if end.startswith('+') else f'{team}/{end}'
            if (b,a) in edges: continue
            road(network[a],network[b],w)
            edges.append((a,b))
    def field(kind,x,z,w,d,yaw=0):
        fields.append(dict(kind=kind,x=x,z=z,w=w,d=d,yaw=round(yaw,5)))
    def marker(key,kind,name,x,z,team=None):
        obj=dict(id=key,kind=kind,name=name,x=x,z=z)
        if team:obj['team']=team
        level['landmarks'].append(obj)
    def thing(kind,x,z,w,d,h,yaw=0,solid=True,**extra):
        props.append(dict(kind=kind,x=round(x,2),z=round(z,2),w=round(w,2),d=round(d,2),h=round(h,2),
                          yaw=round(yaw,5),solid=solid,collisionW=round(extra.pop('collisionW',w),2),
                          collisionD=round(extra.pop('collisionD',d),2),**extra))
    marker('centre','village','Сен-Лоран',1,-7)
    marker('clos','orchard','Яблоневый склон',-150,47)
    marker('highroad','hamlet','Верхняя объездная',45,-172)

    for team,side in [('blue',1),('red',-1)]:
        allied=side==1
        def local(kind,x,z,w,d,h,yaw=0,**kw):
            thing(kind,side*x,side*z,w,d,h,yaw+(0 if allied else math.pi),**kw)
        def area(kind,x,z,w,d,yaw=0):
            field(kind,side*x,side*z,w,d,yaw)
        def site(key,kind,left,right,x,z,owned=False):
            marker(f'{team}-{key}',kind,left if allied else right,side*x,side*z,team if owned else None)
        def stock(kind,x,z,w,d,h,res,amount,yaw=0):
            local(kind,x,z,w,d,h,yaw,resource=res,stock=amount)
        site('base','base','Западный технопарк','Восточный терминал',-232,143,True)
        site('oil','oil','Промысел Бельвю','Промысел Сен-Пьер',-90,-157)
        site('mill','sawmill','Лесокомбинат «Леруа»','Лесокомбинат «Вернье»',-147,157)
        site('quarry','quarry','Карьер «Запад»','Карьер «Восток»',-184,-62)
        site('hamlet','hamlet','Ле-Форж','Ла-Мот',-127,-2)
        site('yard','workyard','Ремонтная база З-1','Ремонтная база В-2',-216,172,True)
        site('orchard','orchard','Сады Тийёль','Сады Бретёй',-150,53)
        yards.append((side*-216,side*172,26,22))
        level['workyards'].append(dict(x=-216*side,z=172*side,w=26,d=22,team=team))
        area('gravel',-216,172,27,23)
        area('gravel',-228,127,52,38)
        area('gravel',-89,-152,29,23,-.1)
        area('earth',-179,-56,41,30,-.1)
        area('gravel',-146,154,33,22,.04)
        area('orchard',-150,56,32,24,.14)
        area('earth',-115,0,32,25,.12)

        # Headquarters: infantry are behind a full-height building, with
        # supplies within a few steps and two clear garage exits.
        for i,(x,z) in enumerate([(-232,141),(-230,144),(-233,146),(-227,142),(-236,143),(-229,147)]):
            level['teamSpawns'][team].append(dict(x=side*x,z=side*z,yaw=round((1.33 if allied else 1.33+math.pi)+(i%3-1)*.07,5)))
        for k,x,z,exit_z in [('patrol4x4',-240,115,128),('cargo6x6',-231,115,128),('mbt',-222,115,128),
                             ('scout_buggy',-240,178,190),('apc',-250,178,189)]:
            level['vehicleSpawns'].append(dict(kind=k,x=x*side,z=z*side,yaw=round(math.pi if allied else 0,6),
                                                team=team,exit={'x':x*side,'z':exit_z*side}))
            road((x*side,z*side),(x*side,exit_z*side),8)
        area('gravel',-245,179,26,15)
        site('motor','garage','Южные боксы','Северные боксы',-245,178,True)
        local('house',-218.5,139,10.4,7.7,8.2,-.17,hp=460,variant=1,style='dispatch')
        local('barn',-244,148,10,8,7.5,.08,hp=330,variant=1,style='serviceHangar')
        local('tower',-251,138,4.6,4.6,10.4,.07,hp=215,style='telecom')
        local('house',-257,93,9,8,6.3,-.25,hp=380,variant=0)
        local('wall',-234,151,7.2,.8,1.2,-.07,hp=130)
        local('sandbag',-245,137,3.6,.85,1.0,.2,hp=120)
        stock('logs',-234,138,2.55,1.65,1.15,'wood',24)
        stock('barrels',-226.1,137.4,1.85,1.6,1.25,'metal',18)
        stock('crate',-235,148,1.6,1.3,.95,'ammo',20)
        stock('crate',-224,146.5,1.7,1.3,1.05,'gunpowder',22)

        # Pump and gantry stand beside, never on, the two access roads.
        local('oilwell',-90,-157,6.2,5.2,9.4,-.14,resource='oil',stock=150)
        local('tower',-99,-163,3.4,3.4,8,.18,hp=180,style='telecom')
        local('barn',-83,-165,7.6,5.2,4.9,.12,hp=280,style='serviceHangar')
        local('barrels',-95,-156,2.4,1.4,1.3,.35)
        local('barrels',-88,-165,2.3,1.5,1.2,-.35)
        local('fence',-101,-158,6.3,.32,1.3,1.4,hp=75)
        # Sawmill and quarry keep visible, collectible supplies close to roads.
        local('barn',-148,156,10,7.6,8.2,-.16,hp=330,variant=1)
        local('logs',-161,161,4.6,2.2,1.6,.32)
        stock('logs',-157,159,4.4,2.7,1.65,'wood',75,.21)
        stock('logs',-141,153,3.6,2.2,1.25,'wood',45,-.2)
        local('house',-137,161,6.2,5.2,5.4,.17,hp=340,variant=3)
        local('barn',-192,-63,9.2,7.5,6.1,.28,hp=340)
        for i,(x,z,w,h) in enumerate([(-181,-66,8.9,4.8),(-190,-75,7.2,4.6),(-199,-65,6.4,2.6),(-197,-51,4.5,2.8)]):
            local('stone',x,z,w,w*.73,h,.64*i)
        stock('barrels',-179,-54,2.4,2.1,1.5,'metal',80,.14)
        stock('crate',-184,-56,1.8,1.3,1.2,'gunpowder',40)
        # Roadside hamlets, a relay tower silhouette and side-lane cover.
        for kind,x,z,w,d,h,a,variant in [
            ('house',-139,5,8.1,7.2,7.6,-.18,1),('ruin',-130,12,7,6.3,6.6,.18,2),
            ('barn',-115,-9,9.2,7.1,6.8,.23,1),('house',-102,18,7.6,6.5,7.2,-.23,0),
            ('house',-63,-96,8,6.9,7.7,.15,2),('tower',-68,-89,3.5,3.5,10.5,.08,1),
            ('ruin',-199,42,8.3,7.8,5.6,.16,2),('barn',-61,82,8.6,6.9,7.2,-.63,1),
            ('house',-40,-178,8.3,6.4,6.7,.25,0),('barn',-19,-186,7.6,8.2,5.7,.38,2),
            ('tower',-231,62,3.7,3.7,8.7,-.14,2),
        ]: local(kind,x,z,w,d,h,a,variant=variant,hp=230 if kind=='tower' else 320)
        stock('crate',-131,5.7,1.4,1.1,1.05,'ammo',15)
        for x,z,l,a in [(-139,10,5,.16),(-103,26,7,-.2),(-64,81,5,.22),
                         (-40,-183,7,.13),(-158,60,8,1.3),(-174,-71,5,.3)]:
            local('fence',x,z,l,.33,1.2,a,hp=70)
        for k,(x,z,w,d,a) in enumerate([
            (-167,94,42,26,.20),(-151,111,32,20,-.16),(-99,129,52,27,.16),
            (-74,146,37,26,.11),(-35,111,33,30,-.25),(12,96,42,41,-.14),
            (-143,33,31,34,.15),(-79,37,29,24,-.24),(-157,-18,30,27,.21),
            (-199,-104,51,30,-.35),(-131,-145,30,31,.18),(-42,-135,37,21,-.3),
            (-26,-71,33,24,-.2),(17,-88,37,28,.19),(34,-142,51,30,.21),
            (-235,-103,36,46,-.34),(-154,-185,49,27,.31),(-89,-202,43,28,-.2),
            (-165,190,34,24,-.18),(-207,201,37,25,.15),(-239,44,33,30,.15),
        ]):area('crop' if k%4!=2 else 'earth',x,z,w,d,a)
    # Outer road verges and scattered woods are seeded, and use the actual
    # oriented footprints: a pretty roadside tree may not cut off a truck.
    rng=random.Random(19440528)
    pickup_sites=[]
    def available(x,z,r,buffer=1):
        if any(point_on_segment_distance(x,z,a,b)<w/2+r+buffer for a,b,w in routes):return False
        if any(abs(x-kx)<w/2+r+2 and abs(z-kz)<d/2+r+2 for kx,kz,w,d in yards):return False
        if any(math.hypot(x-sx,z-sz)<r+2.2 for sx,sz in pickup_sites):return False
        if any(math.hypot(x-v['x'],z-v['z'])<r+5 for v in level['vehicleSpawns']):return False
        return not any(p['solid'] and math.hypot(x-p['x'],z-p['z'])<r+min(8,math.hypot(p['w'],p['d'])/2)+.65 for p in props)
    def free_verge(x,z,r):
        for radius in range(0,29,2):
            for i in range(20):
                a=i*math.tau/20
                px,pz=x+radius*math.cos(a),z+radius*math.sin(a)
                if available(px,pz,r,.15):return round(px,2),round(pz,2)
        raise ValueError(f'No accessible verge near {(x,z)}')

    # Each side has a weighbridge, electrical substation and motorway service
    # stop. Their loading bays sit within walking distance of the road.
    for side,color in [(1,'blue'),(-1,'red')]:
        for key,names,x,z,kind,style in [
            ('scale',('Весовая Лефевр','Весовая Пеллетье'),-191,11,'crate','shippingContainer'),
            ('power',('Подстанция Ланда','Подстанция Маре'),-94,77,'tower','telecom'),
            ('station',('Сервис Шарье','Сервис Роше'),-56,-153,'barn','serviceHangar'),
        ]:
            ax,az=side*x,side*z
            px,pz=free_verge(ax,az,4.7)
            name=names[0 if side==1 else 1]
            marker(f'{color}-{key}','industry',name,px,pz)
            field('gravel',px,pz,24,18,.13)
            for index,(object_kind,object_style,dx,dz,w,d,h,angle) in enumerate([
                (kind,style,0,0,6.2 if kind!='tower' else 3.5,4,7 if kind=='tower' else 3.7,.12),
                ('crate','shippingContainer',-8,1,6.1,2.45,2.6,.12),
                ('barrels','tankBattery' if key=='station' else None,4,6,2.7,2.3,3.7 if key=='station' else 1.4,-.2),
                ('wall',None,3,-7,4.6,.65,1.1,.12),
            ]):
                xx,zz=(px,pz) if index==0 else free_verge(px+side*dx,pz+side*dz,math.hypot(w,d)/2)
                thing(object_kind,xx,zz,w,d,h,angle+(0 if side>0 else math.pi),hp=175,
                      **({'style':object_style} if object_style else {}))
        # These islets are empty physical pick-up areas. The authoritative
        # game supplies stock over time. Icons refer to these exact positions.
        for key,names,mat,x,z,interval,cap in [
            ('base-scrap',('Западный склад крепежа','Восточный склад крепежа'),'metal',-235,134,15,10),
            ('quarry-scrap',('Отвал Западного карьера','Отвал Восточного карьера'),'metal',-176,-57,8,16),
            ('scale-scrap',('Лом у весовой Лефевр','Лом у весовой Пеллетье'),'metal',-190,19,11,12),
            ('mill-timber',('Склад пиломатериалов Леруа','Склад пиломатериалов Вернье'),'wood',-151,137,10,12),
            ('pump-oil',('Отгрузка Бельвю','Отгрузка Сен-Пьер'),'oil',-87,-150,14,12),
            ('power-supply',('Резерв подстанции Ланда','Резерв подстанции Маре'),'powder',-90,70,18,8),
            ('service-fuel',('Топливо у Шарье','Топливо у Роше'),'fuel',-62,-146,25,6),
        ]:
            px,pz=free_verge(side*x,side*z,1.1)
            pickup_sites.append((px,pz))
            level['resourcePoints'].append(dict(id=f'{color}-{key}',name=names[0 if side==1 else 1],
                resource=mat,x=px,z=pz,initial=min(4,cap),capacity=cap,interval=interval))
    def vegetation(kind,x,z,h):
        w=h*(.42 if kind=='pine' else .59)
        thing(kind,x,z,w,w,h,rng.random()*6.28,variant=rng.randrange(4),
              collisionW=h*.069,collisionD=h*.069)
    for side in [1,-1]:
        for cx,cz,n,kind in [(-247,-12,10,'pine'),(-152,55,12,'tree'),(-193,-101,8,'pine'),
                              (-110,193,8,'tree'),(8,-111,8,'tree'),(-54,-193,6,'pine')]:
            accepted=0
            for _ in range(110):
                if accepted>=n:break
                x,z=side*(cx+rng.uniform(-17,17)),side*(cz+rng.uniform(-13,13))
                if available(x,z,2.8):
                    vegetation(kind,x,z,rng.uniform(5.2,9.4) if kind=='tree' else rng.uniform(7.8,13.4))
                    accepted+=1
    for a,b,w in routes[::3]:
        cx,cz,d,yaw=center_of_segment(a,b)
        if d<24:continue
        ortho=(math.cos(yaw),math.sin(yaw))
        for step in [-1,1]:
            x=cx+ortho[0]*(w/2+3.6)*step;z=cz+ortho[1]*(w/2+3.6)*step
            if available(x,z,1.1):
                if step<0:vegetation('tree',x,z,rng.uniform(5.5,9.5))
                else:thing('bush',x,z,3.5,2.1,1.6,.03,False)
    # Terminal yards, pump farms and transshipment depots. These are added
    # after all existing scenery so indexed destruction meshes stay in sync.
    for side in [1,-1]:
        for kind,style,x,z,w,d,h,yaw in [
            ('crate','shippingContainer',-242,162,6.05,2.45,2.59,.09),
            ('crate','shippingContainer',-249,161.5,6.05,2.45,2.59,.09),
            ('crate','shippingContainer',-242,166,6.05,2.45,2.59,.09),
            ('crate','shippingContainer',-248,164.9,6.05,2.45,2.59,.09),
            ('barrels','tankBattery',-94,-176,5.,5.,6.1,0.),
            ('barrels','tankBattery',-101,-176,5.,5.,6.1,0.),
            ('barrels','tankBattery',-108,-176,5.,5.,6.1,0.),
            ('crate','shippingContainer',-170,-65,6.05,2.45,2.59,.3),
            ('crate','shippingContainer',-167,-71,6.05,2.45,2.59,.3)
        ]:
            thing(kind,x*side,z*side,w,d,h,yaw+(0 if side>0 else math.pi),style=style,hp=200)
    level['spawn']=dict(level['teamSpawns']['blue'][0])
    level['routeGraph']={'nodes':{k:dict(x=p[0],z=p[1]) for k,p in network.items()},'edges':edges}
    return level,routes

if __name__=='__main__':
    data,_=generate()
    OUT.write_text(json.dumps(data,ensure_ascii=False,separators=(',',':'))+'\n')
    print(f"{OUT.name}: {len(data['props'])} props, {len(data['roads'])} roads, {len(data['fields'])} land parcels")
